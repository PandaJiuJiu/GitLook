//! Session 管理：cookie + SQLite 存储。
//!
//! - 登录成功 -> 生成 32 字节随机 token（base64url 编码）
//! - Set-Cookie: `gitlook_session=<token>; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000`
//! - token 存 `sessions` 表，`expires_at = now + 30 天`
//! - 受保护路由中间件：读 cookie -> 查 DB（顺便检查过期）-> 拿 `user_id` 注入 request extensions
//! - 登出：从 DB 删 session 行 + 清 cookie
//! - 改密：删该用户所有 session（强制重新登录）

use axum::{
    extract::{FromRequestParts, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use anyhow::Result;
use base64::Engine;

use crate::server::AppState;

/// 经过认证的用户（从 session 解析出的最小信息）。
#[derive(Debug, Clone)]
pub struct SessionUser {
    pub user_id: i64,
    pub username: String,
}

/// 从受保护路由中提取当前用户。
///
/// 用法：`async fn handler(AuthUser(user): AuthUser, ...)`
pub struct AuthUser(pub SessionUser);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // 从 extensions 里取（由 require_session 中间件放进去）
        if let Some(user) = parts.extensions.get::<SessionUser>() {
            return Ok(AuthUser(user.clone()));
        }

        // 没有 extensions 里的说明中间件没跑或没通过；返回 401
        Err((
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"success":false,"error":"unauthorized","data":null}"#,
        )
            .into_response())
    }
}

/// 生成 32 字节随机 session token（base64url，无 padding）。
pub fn generate_token() -> String {
    use rand::{rngs::OsRng, RngCore};
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Session cookie 名称。
pub const SESSION_COOKIE_NAME: &str = "gitlook_session";

/// Session 过期天数。
pub const SESSION_EXPIRY_DAYS: i64 = 30;

/// 从 request headers 里提取 session cookie 值。
pub fn extract_session_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some((name, value)) = pair.split_once('=') {
            if name == SESSION_COOKIE_NAME {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// 设置 session cookie（登录成功时调用）。
pub fn set_session_cookie(resp: &mut Response, token: &str) {
    use axum::http::header::SET_COOKIE;
    use axum::http::HeaderValue;

    // Max-Age = 30 天，HttpOnly，SameSite=Strict，Path=/
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_COOKIE_NAME,
        token,
        SESSION_EXPIRY_DAYS * 86400
    );
    resp.headers_mut().insert(SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
}

/// 清除 session cookie（登出时调用）。
pub fn clear_session_cookie(resp: &mut Response) {
    use axum::http::header::SET_COOKIE;
    use axum::http::HeaderValue;

    let cookie = format!(
        "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        SESSION_COOKIE_NAME
    );
    resp.headers_mut().insert(SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
}

/// 保护路由的中间件：校验 cookie -> 查 DB -> 注入 SessionUser 到 extensions。
///
/// - 对 `/api/*`：失败返回 401 JSON
/// - 对 `/admin/*`：失败返回 302 到 `/login?next=...`
pub async fn require_session(
    State(state): State<AppState>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    use axum::http::{header, StatusCode, Uri};

    let Some(token) = extract_session_token(request.headers()) else {
        return unauthorized_response(request.uri().path()).await;
    };

    // 查 DB：token 必须存在且未过期
    let user = match state.db.query(move |conn: &mut rusqlite::Connection| -> anyhow::Result<SessionUser> {
        let mut stmt = conn.prepare(
            "SELECT u.id, u.username
             FROM sessions s
             JOIN users u ON u.id = s.user_id
             WHERE s.token = ?1 AND s.expires_at > strftime('%s', 'now')"
        )?;
        let row = stmt.query_row([&token], |row| {
            Ok(SessionUser {
                user_id: row.get(0)?,
                username: row.get(1)?,
            })
        });
        Ok(row?)
    }).await {
        Ok(u) => u,
        Err(_) => return unauthorized_response(request.uri().path()).await,
    };

    // 注入 extensions，后续 handler 可通过 AuthUser 提取
    request.extensions_mut().insert(user);

    next.run(request).await
}

/// 生成 401 或 302 响应，视路径而定。
async fn unauthorized_response(path: &str) -> Response {
    use axum::http::{header, StatusCode};

    if path.starts_with("/api/") {
        // API 返回 401 JSON
        return (
            StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"success":false,"error":"unauthorized","data":null}"#,
        )
            .into_response();
    }

    // 其它（主要是 /admin）重定向到登录页，保留 next 参数
    let next = percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC).to_string();
    let location = format!("/login?next={}", next);
    (
        StatusCode::FOUND,
        [(header::LOCATION, location)],
        "",
    )
        .into_response()
}