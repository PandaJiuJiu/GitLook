use figment::providers::{Env, Format, Toml};
use figment::{Figment, providers::Serialized};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub git: GitConfig,
    pub static_files: StaticConfig,
    pub webhook: WebhookConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub base_path: String,
    pub max_body_size: usize,
    pub request_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    pub repos_dir: PathBuf,
    pub worktrees_dir: PathBuf,
    pub default_branch: String,
    pub hook_template: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticConfig {
    pub auto_index: bool,
    pub index_template: PathBuf,
    pub spa_fallback: bool,
    pub cache_max_age: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub secret: String,
    pub allowed_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub json_format: bool,
    pub file_output: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            server: ServerConfig {
                host: "0.0.0.0".into(),
                port: 9999,
                base_path: "".into(),
                max_body_size: 100 * 1024 * 1024, // 100MB
                request_timeout_secs: 300,
            },
            git: GitConfig {
                repos_dir: home.join("minisite/repos"),
                worktrees_dir: home.join("minisite/worktrees"),
                default_branch: "main".into(),
                hook_template: PathBuf::from("hooks/post-receive"),
            },
            static_files: StaticConfig {
                auto_index: true,
                index_template: PathBuf::from("templates/dir_index.html.tera"),
                spa_fallback: false,
                cache_max_age: 3600,
            },
            webhook: WebhookConfig {
                enabled: false,
                secret: "".into(),
                allowed_events: vec!["push".into()],
            },
            logging: LoggingConfig {
                level: "info".into(),
                json_format: false,
                file_output: None,
            },
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let figment = Figment::new()
            .merge(Toml::file("config.toml").nested())
            .merge(Env::prefixed("MINISITE_").global().split("_"))
            .merge(Serialized::defaults(Config::default()));

        let config: Config = figment.extract()?;

        let expand = |p: PathBuf| -> PathBuf {
            if p.starts_with("~") {
                if let Some(home) = dirs::home_dir() {
                    return home.join(p.strip_prefix("~").unwrap());
                }
            }
            p
        };

        Ok(Config {
            git: GitConfig {
                repos_dir: expand(config.git.repos_dir),
                worktrees_dir: expand(config.git.worktrees_dir),
                ..config.git
            },
            static_files: StaticConfig {
                index_template: expand(config.static_files.index_template),
                ..config.static_files
            },
            logging: LoggingConfig {
                file_output: config.logging.file_output.map(expand),
                ..config.logging
            },
            ..config
        })
    }

    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    pub fn base_path(&self) -> &str {
        self.server.base_path.trim_end_matches('/')
    }
}