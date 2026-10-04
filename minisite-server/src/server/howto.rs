//! HowTo 端点：返回 Markdown 使用说明，AI/脚本一次 curl 即可掌握用法

use axum::{http::header, response::Response};

/// 返回 Markdown 格式的 HowTo
///
/// 内容在编译期从 `docs/HOWTO.md` 内嵌进二进制，运行时无需读取磁盘。
pub async fn howto_markdown() -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
        .body(axum::body::Body::from(include_str!("../../../docs/HOWTO.md")))
        .unwrap()
}