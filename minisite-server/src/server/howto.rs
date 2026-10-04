//! HowTo 端点：在 GET /howto 返回 Markdown 使用说明，供 AI/脚本自述用途。
//!
//! 文档是磁盘上的普通 .md 文件（配置项 `server.howto_file`），不在编译期内嵌，
//! 因此运维可以直接编辑部署目录里的文档，改动无需重新编译或重启。

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::fs;

use crate::server::AppState;

/// 已缓存的文档内容及其来源文件的 mtime
#[derive(Clone)]
struct Cached {
    mtime: Option<SystemTime>,
    body: String,
}

/// 带 mtime 校验的文档缓存
#[derive(Clone)]
pub struct HowTo {
    path: PathBuf,
    cache: Arc<Mutex<Option<Cached>>>,
}

impl HowTo {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// 读取文档。文件 mtime 未变则复用缓存，避免每次请求都打磁盘。
    async fn load(&self) -> String {
        let current_mtime = fs::metadata(&self.path)
            .await
            .ok()
            .and_then(|m| m.modified().ok());

        if let Ok(cache) = self.cache.lock() {
            if let Some(c) = cache.as_ref() {
                if c.mtime == current_mtime {
                    return c.body.clone();
                }
            }
        }

        let body = match fs::read_to_string(&self.path).await {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(
                    "Failed to read howto file {}: {}. Serving fallback instead.",
                    self.path.display(),
                    e
                );
                format!(
                    "# minisite — HowTo unavailable\n\n\
                     Could not read the HowTo document at `{}`.\n\n\
                     Reason: {}\n\n\
                     Set `server.howto_file` in config.toml to a readable Markdown file.\n",
                    self.path.display(),
                    e
                )
            }
        };

        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(Cached {
                mtime: current_mtime,
                body: body.clone(),
            });
        }

        body
    }
}

pub async fn howto(State(state): State<AppState>) -> Response {
    let body = state.howto.load().await;

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        body,
    )
        .into_response()
}