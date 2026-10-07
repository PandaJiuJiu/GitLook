//! SQLite 数据库层：��户表 + session 表。
//!
//! 启动时自动建表（`CREATE TABLE IF NOT EXISTS`）。
//! 通过 `tokio::sync::Mutex<Connection>` + `spawn_blocking` 提供异步接口。

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::auth::session::SESSION_EXPIRY_DAYS;

/// 数据库封装：内部持有连接，对外提供 `query` 方法运行同步回调。
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// 打开数据库文件，初始化 schema。
    pub async fn open(path: &Path) -> Result<Self> {
        let path = path.to_owned();
        let conn = tokio::task::spawn_blocking(move || -> Result<Connection> {
            let conn = Connection::open(&path)
                .with_context(|| format!("Cannot open SQLite at {}", path.display()))?;
            conn.execute_batch(
                r#"
                PRAGMA foreign_keys = ON;
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
                PRAGMA busy_timeout = 5000;
                "#,
            )?;
            Ok(conn)
        })
        .await??;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema().await?;
        Ok(db)
    }

    /// 建表（幂等）。
    async fn init_schema(&self) -> Result<()> {
        self.query(|conn| {
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS users (
                    id            INTEGER PRIMARY KEY AUTOINCREMENT,
                    username      TEXT    NOT NULL UNIQUE,
                    password_hash TEXT    NOT NULL,
                    created_at    INTEGER NOT NULL,
                    last_login_at INTEGER
                );

                CREATE TABLE IF NOT EXISTS sessions (
                    token       TEXT    PRIMARY KEY,
                    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    created_at  INTEGER NOT NULL,
                    expires_at  INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);
                "#,
            )?;
            Ok(())
        })
        .await
    }

    /// 运行同步数据库操作（在 spawn_blocking 里执行）。
    pub async fn query<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.blocking_lock();
            f(&mut conn)
        })
        .await?
    }

    // ===== Users =====

    /// 查询用户总数（用于判断是否首次运行）。
    pub async fn count_users(&self) -> Result<i64> {
        self.query(|conn| {
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM users")?;
            let count: i64 = stmt.query_row([], |row| row.get(0))?;
            Ok(count)
        })
        .await
    }

    /// 按用户名查用户（返回 id 和 password_hash）。
    pub async fn find_user_by_name(&self, username: &str) -> Result<Option<(i64, String)>> {
        let username = username.to_owned();
        self.query(move |conn| {
            let mut stmt = conn.prepare("SELECT id, password_hash FROM users WHERE username = ?1")?;
            let row = stmt.query_row([username], |row| Ok((row.get(0)?, row.get(1)?)));
            match row {
                Ok(u) => Ok(Some(u)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        })
        .await
    }

    /// 创建用户，返回新 id。
    pub async fn create_user(&self, username: &str, password_hash: &str) -> Result<i64> {
        let username = username.to_owned();
        let password_hash = password_hash.to_owned();
        let now = chrono::Utc::now().timestamp();
        self.query(move |conn| {
            conn.execute(
                "INSERT INTO users (username, password_hash, created_at) VALUES (?1, ?2, ?3)",
                params![username, password_hash, now],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await
    }

    /// 更新用户密码（同时删除该用户所有 session）。
    pub async fn update_password(&self, user_id: i64, new_hash: &str) -> Result<()> {
        let new_hash = new_hash.to_owned();
        self.query(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE users SET password_hash = ?1 WHERE id = ?2",
                params![new_hash, user_id],
            )?;
            tx.execute("DELETE FROM sessions WHERE user_id = ?1", params![user_id])?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 更新用户最后登录时间。
    pub async fn update_last_login(&self, user_id: i64) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        self.query(move |conn| {
            conn.execute(
                "UPDATE users SET last_login_at = ?1 WHERE id = ?2",
                params![now, user_id],
            )?;
            Ok(())
        })
        .await
    }

    /// 列出所有用户（管理页面用）。
    pub async fn list_users(&self) -> Result<Vec<(i64, String, i64, Option<i64>)>> {
        self.query(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, username, created_at, last_login_at FROM users ORDER BY id"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })?;
            let mut users = Vec::new();
            for row in rows {
                users.push(row?);
            }
            Ok(users)
        })
        .await
    }

    /// 删除用户（级联删 session）。
    pub async fn delete_user(&self, user_id: i64) -> Result<()> {
        self.query(move |conn| {
            conn.execute("DELETE FROM users WHERE id = ?1", params![user_id])?;
            Ok(())
        })
        .await
    }

    // ===== Sessions =====

    /// 创建 session（登录成功时调用）。
    pub async fn create_session(&self, token: &str, user_id: i64) -> Result<()> {
        let token = token.to_owned();
        let now = chrono::Utc::now().timestamp();
        let expires_at = now + SESSION_EXPIRY_DAYS * 86400;
        self.query(move |conn| {
            conn.execute(
                "INSERT INTO sessions (token, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
                params![token, user_id, now, expires_at],
            )?;
            Ok(())
        })
        .await
    }

    /// 删除 session（登出时调用）。
    pub async fn delete_session(&self, token: &str) -> Result<()> {
        let token = token.to_owned();
        self.query(move |conn| {
            conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
            Ok(())
        })
        .await
    }

    /// 删除用户所有 session（改密时调用）。
    pub async fn delete_user_sessions(&self, user_id: i64) -> Result<()> {
        self.query(move |conn| {
            conn.execute("DELETE FROM sessions WHERE user_id = ?1", params![user_id])?;
            Ok(())
        })
        .await
    }

    /// 清理过期 session（可选，定期或启动时跑一次）。
    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let now = chrono::Utc::now().timestamp();
        self.query(move |conn| {
            let deleted = conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", params![now])?;
            Ok(deleted as u64)
        })
        .await
    }
}