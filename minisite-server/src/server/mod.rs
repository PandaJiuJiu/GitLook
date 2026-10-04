mod api;
mod howto;
mod static_files;

use crate::config::Config;
use crate::git::GitManager;
use crate::server::static_files::StaticFileServer;
use anyhow::Result;
use axum::{
    Router,
    extract::DefaultBodyLimit,
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
        config.static_files.spa_fallback,
        config.static_files.cache_max_age,
    )?);

    let state = AppState {
        git: git_manager,
        static_files: static_server,
    };

    // CORS 配置
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    // 构建路由
    let app = Router::new()
        // 根入口
        .route("/", get(root_info))
        // HowTo 端点（供 AI/脚本使用）
        .route("/howto", get(howto::howto_markdown))
        .route("/howto.md", get(howto::howto_markdown))
        .route("/howto.json", get(howto::howto_json))
        .route("/howto.txt", get(howto::howto_plain))
        // API 路由
        .route("/api/repos", get(api::list_repos))
        .route("/api/repos", post(api::create_repo))
        .route("/api/repos/{name}", delete(api::delete_repo))
        .route("/api/repos/{name}/deploy", post(api::trigger_deploy))
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

async fn root_info() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "service": "minisite",
        "version": env!("CARGO_PKG_VERSION"),
        "howto": {
            "markdown": "/howto",
            "json": "/howto.json",
            "plain": "/howto.txt"
        },
        "endpoints": {
            "health": "/health",
            "api": {
                "list_repos": "GET /api/repos",
                "create_repo": "POST /api/repos",
                "delete_repo": "DELETE /api/repos/{name}",
                "deploy_repo": "POST /api/repos/{name}/deploy"
            }
        }
    }))
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