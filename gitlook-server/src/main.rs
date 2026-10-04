mod config;
mod git;
mod server;

use anyhow::Result;
use clap::Parser;
use gitlook_server::config::Config;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "gitlook", version, about = "Lightweight self-hosted static site hosting with Git push deployment")]
struct Args {
    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Server host
    #[arg(long)]
    host: Option<String>,

    /// Server port
    #[arg(short, long)]
    port: Option<u16>,

    /// Git repositories directory
    #[arg(long)]
    repos_dir: Option<String>,

    /// Git worktrees directory
    #[arg(long)]
    worktrees_dir: Option<String>,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Initialize configuration file
    #[arg(long)]
    init_config: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 初始化日志
    let log_level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(log_level.parse()?))
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    // 初始化配置文件
    if args.init_config {
        return init_config_file(&args.config).await;
    }

    // 加载配置
    let mut config = Config::load(&args.config)?;

    // 命令行参数覆盖
    if let Some(host) = args.host {
        config.server.host = host;
    }
    if let Some(port) = args.port {
        config.server.port = port;
    }
    if let Some(repos_dir) = args.repos_dir {
        config.git.repos_dir = repos_dir.into();
    }
    if let Some(worktrees_dir) = args.worktrees_dir {
        config.git.worktrees_dir = worktrees_dir.into();
    }

    info!("gitlook starting...");
    info!("Config file: {}", args.config);
    info!("Config: {:?}", config);

    // 运行服务器
    gitlook_server::server::run(config).await?;

    Ok(())
}

async fn init_config_file(path: &str) -> Result<()> {
    use std::fs;

    let config = Config::default();
    let toml = toml::to_string_pretty(&config)?;
    let commented = format!(
        "# gitlook Configuration\n# Generated at {}\n\n{}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        toml
    );

    fs::write(path, commented)?;
    println!("Configuration file created at: {}", path);
    println!("Edit it and run again without --init-config to start the server.");
    Ok(())
}