use anyhow::Result;
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use mime_guess::from_path;
use std::{
    path::{Path as StdPath, PathBuf},
    sync::Arc,
};
use tera::{Context as TeraContext, Tera};
use tokio::fs;

use crate::server::AppState;

#[derive(Clone)]
pub struct StaticFileServer {
    worktrees_dir: PathBuf,
    base_path: String,
    auto_index: bool,
    spa_fallback: bool,
    cache_max_age: u64,
    tera: Arc<Tera>,
}

impl StaticFileServer {
    pub fn new(
        worktrees_dir: PathBuf,
        base_path: String,
        auto_index: bool,
        index_template: PathBuf,
        spa_fallback: bool,
        cache_max_age: u64,
    ) -> Result<Self> {
        let mut tera = Tera::default();
        if index_template.exists() {
            tera.add_template_file(&index_template, Some("dir_index"))?;
        } else {
            // 内置模板
            tera.add_raw_template("dir_index", BUILTIN_INDEX_TEMPLATE)?;
        }
        tera.autoescape_on(vec![]);

        Ok(Self {
            worktrees_dir,
            base_path,
            auto_index,
            spa_fallback,
            cache_max_age,
            tera: Arc::new(tera),
        })
    }

    /// 构建路由
    pub fn router() -> Router<AppState> {
        Router::new()
            .route("/{repo}/", get(serve_repo_root))
            .route("/{repo}/{*path}", get(serve_repo_file_handler))
    }
}

async fn serve_repo_root(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
) -> Response {
    serve_repo_file_inner(&state.static_files, &repo, "").await
}

async fn serve_repo_file_handler(
    State(state): State<AppState>,
    AxumPath((repo, path)): AxumPath<(String, String)>,
) -> Response {
    serve_repo_file_inner(&state.static_files, &repo, &path).await
}

async fn serve_repo_file_inner(
    server: &StaticFileServer,
    repo: &str,
    path: &str,
) -> Response {
    let repo_path = server.worktrees_dir.join(repo);

    // 检查仓库是否存在
    if !repo_path.exists() || !repo_path.is_dir() {
        return not_found_response(&format!("Repository '{}' not found", repo));
    }

    // 防止路径遍历
    let requested_path = if path.is_empty() {
        repo_path.clone()
    } else {
        repo_path.join(path)
    };

    // 规范化路径并检查是否在仓库目录内
    let requested_path = match requested_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return not_found_response("Path not found"),
    };

    let repo_canonical = match repo_path.canonicalize() {
        Ok(p) => p,
        Err(_) => return not_found_response("Repository not accessible"),
    };

    if !requested_path.starts_with(&repo_canonical) {
        return not_found_response("Path not found");
    }

    // 如果是目录
    if requested_path.is_dir() {
        return serve_directory(server, repo, path, &requested_path).await;
    }

    // 服务文件
    serve_file(server, &requested_path, path).await
}

async fn serve_directory(
    server: &StaticFileServer,
    repo: &str,
    request_path: &str,
    dir_path: &StdPath,
) -> Response {
    // index.html 优先。静态站托管的常态是目录里带首页，这时给用户一个文件
    // 列表而不是站点本身显然不对。auto_index 只决定"没有 index.html 时要不要
    // 生成列表"，不该决定 index.html 算不算数。
    let index_path = dir_path.join("index.html");
    if index_path.is_file() {
        let href = format!("{}/index.html", request_path.trim_end_matches('/'));
        return serve_file(server, &index_path, &href).await;
    }

    if !server.auto_index {
        if server.spa_fallback {
            let spa_index = server.worktrees_dir.join(repo).join("index.html");
            if spa_index.exists() {
                return serve_file(server, &spa_index, "index.html").await;
            }
        }
        return not_found_response("Directory index not found");
    }

    // 生成目录索引
    let mut entries = Vec::new();
    let mut read_dir = match fs::read_dir(dir_path).await {
        Ok(d) => d,
        Err(_) => return not_found_response("Cannot read directory"),
    };

    // 父目录链接
    if !request_path.is_empty() {
        let parent_path = StdPath::new(request_path)
            .parent()
            .unwrap_or(StdPath::new(""))
            .to_string_lossy()
            .to_string();
        entries.push(DirEntry {
            name: "..".to_string(),
            href: format!("/{}/{}/", repo, parent_path.trim_start_matches('/')),
            is_dir: true,
            size_display: "—".to_string(),
            modified_display: "—".to_string(),
        });
    }

    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".." {
            continue; // 跳过隐藏文件
        }

        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_dir = metadata.is_dir();
        let href = if is_dir {
            format!("/{}/{}/", repo, StdPath::new(request_path).join(&name).to_string_lossy().trim_start_matches('/'))
        } else {
            format!("/{}/{}", repo, StdPath::new(request_path).join(&name).to_string_lossy().trim_start_matches('/'))
        };

        entries.push(DirEntry {
            name,
            href,
            is_dir,
            size_display: if is_dir {
                "—".to_string()
            } else {
                human_size(metadata.len())
            },
            modified_display: metadata
                .modified()
                .ok()
                .map(|t| {
                    chrono::DateTime::<chrono::Utc>::from(t)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "—".to_string()),
        });
    }

    // 排序：目录在前，然后按名称
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    // 生成面包屑路径
    let breadcrumbs: Vec<Breadcrumb> = if request_path.is_empty() {
        vec![]
    } else {
        let mut crumbs = vec![Breadcrumb {
            name: repo.to_string(),
            href: format!("/{}/", repo),
        }];
        let parts: Vec<&str> = request_path.split('/').filter(|s| !s.is_empty()).collect();
        let mut accumulated = String::new();
        for part in parts {
            if !accumulated.is_empty() {
                accumulated.push('/');
            }
            accumulated.push_str(part);
            crumbs.push(Breadcrumb {
                name: part.to_string(),
                href: format!("/{}/{}/", repo, accumulated),
            });
        }
        crumbs
    };

    let mut tera_ctx = TeraContext::new();
    tera_ctx.insert("repo", repo);
    tera_ctx.insert("path", request_path);
    tera_ctx.insert("entries", &entries);
    tera_ctx.insert("breadcrumbs", &breadcrumbs);

    let html = match server.tera.render("dir_index", &tera_ctx) {
        Ok(html) => html,
        Err(e) => {
            tracing::error!("Failed to render dir_index template for /{}/{}: {:?}", repo, request_path, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                format!("Template render error for /{}/{}: {}", repo, request_path, e),
            )
                .into_response();
        }
    };

    let mut resp = Html(html).into_response();
    set_cache_headers(&mut resp, server.cache_max_age);
    resp
}

#[derive(serde::Serialize)]
struct DirEntry {
    name: String,
    href: String,
    is_dir: bool,
    /// 预渲染好的展示字符串（避免向模板传 null，Tera 处理 null 不可靠）
    size_display: String,
    modified_display: String,
}

/// 字节数转为人类可读格式
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

#[derive(serde::Serialize)]
struct Breadcrumb {
    name: String,
    href: String,
}

async fn serve_file(server: &StaticFileServer, file_path: &StdPath, _request_path: &str) -> Response {
    let mime = from_path(file_path).first_or_octet_stream();

    let file = match fs::File::open(file_path).await {
        Ok(f) => f,
        Err(_) => return not_found_response("File not found"),
    };

    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(_) => return not_found_response("File metadata error"),
    };

    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CONTENT_LENGTH, metadata.len())
        .body(body)
        .unwrap();

    // ETag 基于文件大小和修改时间
    let modified = metadata.modified().ok().map(|t| chrono::DateTime::<chrono::Utc>::from(t));
    if let Some(m) = modified {
        let etag = format!("\"{:x}-{:x}\"", metadata.len(), m.timestamp());
        resp.headers_mut().insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
        resp.headers_mut().insert(
            header::LAST_MODIFIED,
            HeaderValue::from_str(&m.format("%a, %d %b %Y %H:%M:%S GMT").to_string()).unwrap(),
        );
    }

    set_cache_headers(&mut resp, server.cache_max_age);
    resp
}

fn set_cache_headers(resp: &mut Response, max_age: u64) {
    let cache_control = format!("public, max-age={}, stale-while-revalidate={}", max_age, max_age * 24);
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_str(&cache_control).unwrap());
}

fn not_found_response(msg: &str) -> Response {
    let mut resp = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(msg.to_string()))
        .unwrap();
    resp
}

const BUILTIN_INDEX_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Index of /{{ repo }}/{{ path }}</title>
    <style>
        * { box-sizing: border-box; }
        body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; max-width: 900px; margin: 0 auto; padding: 2rem; line-height: 1.6; color: #333; }
        h1 { color: #2c3e50; border-bottom: 2px solid #3498db; padding-bottom: 0.5rem; }
        table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
        th, td { padding: 0.75rem 1rem; text-align: left; border-bottom: 1px solid #eee; }
        th { background: #f8f9fa; font-weight: 600; color: #495057; }
        tr:hover td { background: #f8f9fa; }
        a { color: #3498db; text-decoration: none; }
        a:hover { text-decoration: underline; }
        .dir a::before { content: "📁 "; }
        .file a::before { content: "📄 "; }
        .parent a::before { content: "⬆️ "; }
        .size { color: #6c757d; font-size: 0.9rem; white-space: nowrap; }
        .modified { color: #6c757d; font-size: 0.85rem; white-space: nowrap; }
        .empty { color: #adb5bd; font-style: italic; padding: 2rem; text-align: center; }
        .repo-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 1rem; }
        .repo-name { font-size: 1.5rem; font-weight: 600; color: #2c3e50; }
        .breadcrumb { color: #6c757d; font-size: 0.9rem; }
        .breadcrumb a { color: #6c757d; }
        .breadcrumb a:hover { color: #3498db; }
    </style>
</head>
<body>
    <div class="repo-header">
        <div class="repo-name">{{ repo }}</div>
        <div class="breadcrumb">
            {% for crumb in breadcrumbs %}
                {% if loop.first %}{% else %} / {% endif %}<a href="{{ crumb.href }}">{{ crumb.name }}</a>
            {% endfor %}
            {% if path == "" %}/{% endif %}
        </div>
    </div>

    <h1>Index of /{{ repo }}/{{ path }}</h1>

    {% if entries %}
        <table>
            <thead>
                <tr>
                    <th style="width: 50%;">Name</th>
                    <th style="width: 15%;">Size</th>
                    <th style="width: 35%;">Modified</th>
                </tr>
            </thead>
            <tbody>
                {% for entry in entries %}
                {% set row_class = "file" %}
                {% if entry.is_dir %}{% set row_class = "dir" %}{% endif %}
                {% if entry.name == ".." %}{% set row_class = "parent" %}{% endif %}
                <tr class="{{ row_class }}">
                    <td><a href="{{ entry.href }}">{{ entry.name }}</a></td>
                    <td class="size">{{ entry.size_display }}</td>
                    <td class="modified">{{ entry.modified_display }}</td>
                </tr>
                {% endfor %}
            </tbody>
        </table>
    {% else %}
        <p class="empty">Directory is empty</p>
    {% endif %}
</body>
</html>
"#;