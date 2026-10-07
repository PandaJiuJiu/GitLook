mod api;
mod home;
mod howto;
pub mod static_files;

use crate::auth::{handlers, session::require_session};
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
    /// 数据库连接（用户 + session）
    pub db: Arc<crate::auth::db::Database>,
    /// 首页 footer 里的 GitHub 链接
    pub github_url: String,
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
        config.static_files.setup_template.clone(),
        config.static_files.login_template.clone(),
        config.static_files.admin_template.clone(),
        config.static_files.spa_fallback,
        config.static_files.cache_max_age,
    )?);

    // 打开数据库（用户 + session）
    let db = Arc::new(crate::auth::db::Database::open(&config.git.db_path).await?);
    // 启动时清理过期 session
    if let Err(e) = db.cleanup_expired_sessions().await {
        tracing::warn!("Failed to cleanup expired sessions: {}", e);
    }

    let state = AppState {
        git: git_manager,
        static_files: static_server,
        howto: HowTo::new(config.server.howto_file.clone()),
        db,
        github_url: config.server.github_url.clone(),
    };

    // CORS 配置
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    // API 路由单独成一个 Router，才能用 route_layer 只给 API 加鉴权。
    // 直接 layer 到总路由上会把静态文件和 /howto 一起挡住。
    let state_for_middleware = state.clone();
    let api = Router::new()
        .route("/api/repos", get(api::list_repos))
        .route("/api/repos", post(api::create_repo))
        .route("/api/repos/{name}", delete(api::delete_repo))
        .route("/api/repos/{name}/deploy", post(api::trigger_deploy))
        .route_layer(axum::middleware::from_fn(move |request, next| {
            let state = state_for_middleware.clone();
            async move { require_session(axum::extract::State(state), request, next).await }
        }));

    // 认证路由（公开）
    let auth_public = Router::new()
        .route("/setup", get(handlers::setup_get).post(handlers::setup_post))
        .route("/login", get(handlers::login_get).post(handlers::login_post));

    // 认证路由（需要登录）
    let state_for_middleware2 = state.clone();
    let auth_protected = Router::new()
        .route("/logout", post(handlers::logout_post))
        .route("/admin", get(handlers::admin_get).post(handlers::admin_user_post))
        .route("/admin/password", post(handlers::admin_password_post))
        .route("/admin/users/{id}", delete(handlers::admin_user_delete))
        .route_layer(axum::middleware::from_fn(move |request, next| {
            let state = state_for_middleware2.clone();
            async move { require_session(axum::extract::State(state), request, next).await }
        }));

    // 构建路由
    let app = Router::new()
        // 根入口
        .route("/", get(home::home))
        // HowTo 端点（供 AI/脚本使用）
        .route("/howto", get(howto::howto))
        // 公开认证路由
        .merge(auth_public)
        // 受保护认证路由
        .merge(auth_protected)
        // API 路由
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