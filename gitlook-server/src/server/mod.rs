mod api;
mod home;
mod howto;
mod static_files;

use crate::config::Config;
use crate::git::GitManager;
use crate::server::{howto::HowTo, static_files::StaticFileServer};
use anyhow::Result;
use axum::{
    Router,
    extract::{DefaultBodyLimit, State},
    http::Method,
    routing::{get, post, delete},
};
use std::sync::Arc;
use tokio::signal;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub git: Arc<GitManager>,
    pub static_files: Arc<StaticFileServer>,
    pub howto: HowTo,
    /// 保护 /api/* 的 token；None 表示不鉴权
    pub api_token: Option<Arc<String>>,
    /// 首页 footer 里的 GitHub 链接
    pub github_url: String,
}

/// 校验 `Authorization: Bearer <token>`。
///
/// 只挂在 /api/* 上：站点和 /howto 是公开的，鉴权要保护的是能建仓库、
/// 删仓库的那些接口，不是内容本身。
///
/// 用 route_layer 而不是 layer：只有真的匹配到 API 路由的请求才会经过这里，
/// 未匹配的路径直接透传，静态文件不受影响。
async fn require_api_token(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::{http::header, response::IntoResponse};

    let Some(expected) = state.api_token.as_deref() else {
        return next.run(request).await;
    };

    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);

    // 常量时间比较，避免通过响应时间逐字节猜 token
    let ok = match provided {
        Some(t) => {
            let a = t.as_bytes();
            let b = expected.as_bytes();
            a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
        }
        None => false,
    };

    if !ok {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            [(header::CONTENT_TYPE, "application/json")],
            r#"{"success":false,"error":"missing or invalid Authorization: Bearer <token>","data":null}"#,
        )
            .into_response();
    }

    next.run(request).await
}

pub async fn run(config: Config) -> Result<()> {
    // 初始化 Git 管理器
    let git_manager = Arc::new(GitManager::new(
        config.git.repos_dir.clone(),
        config.git.worktrees_dir.clone(),
        config.git.default_branch.clone(),
        config.git.hook_template.clone(),
    ).await?);

    // 初始化静态文件服务器
    let static_server = Arc::new(StaticFileServer::new(
        config.git.worktrees_dir.clone(),
        config.base_path().to_string(),
        config.static_files.auto_index,
        config.static_files.index_template.clone(),
        config.static_files.home_template.clone(),
        config.static_files.spa_fallback,
        config.static_files.cache_max_age,
    )?);

    let state = AppState {
        git: git_manager,
        static_files: static_server,
        howto: HowTo::new(config.server.howto_file.clone()),
        api_token: config.server.api_token.clone().map(Arc::new),
        github_url: config.server.github_url.clone(),
    };

    if state.api_token.is_none() {
        tracing::warn!(
            "No API token configured (server.api_token or MINISITE_API_TOKEN). \
             /api/* is open to anyone who can reach this port — anyone can create \
             or delete repositories. Set a token before exposing it beyond loopback."
        );
    } else {
        info!("API token loaded; /api/* requires Authorization: Bearer <token>");
    }

    // CORS 配置
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    // API 路由单独成一个 Router，才能用 route_layer 只给 API 加鉴权。
    // 直接 layer 到总路由上会把静态文件和 /howto 一起挡住。
    let api = Router::new()
        .route("/api/repos", get(api::list_repos))
        .route("/api/repos", post(api::create_repo))
        .route("/api/repos/{name}", delete(api::delete_repo))
        .route("/api/repos/{name}/deploy", post(api::trigger_deploy))
        .route_layer(axum::middleware::from_fn_with_state(state.clone(), require_api_token));

    // 构建路由
    let app = Router::new()
        // 根入口
        .route("/", get(home::home))
        // HowTo 端点（供 AI/脚本使用）
        .route("/howto", get(howto::howto))
        .merge(api)
        // 静态文件路由
        .merge(StaticFileServer::router())
        // 健康检查
        .route("/health", get(health_check))
        // 中间件
        .layer(DefaultBodyLimit::max(config.server.max_body_size))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state);

    let addr = config.server_addr();
    info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server stopped gracefully");
    Ok(())
}

async fn health_check() -> &'static str {
    "ok"
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("Received Ctrl+C, shutting down..."),
        _ = terminate => info!("Received SIGTERM, shutting down..."),
    }
}