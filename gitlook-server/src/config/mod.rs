use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    pub git: GitConfig,
    pub static_files: StaticConfig,
    pub webhook: WebhookConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub base_path: String,
    pub max_body_size: usize,
    pub request_timeout_secs: u64,
    /// 供 AI/脚本自述用途的 Markdown 文档，在 GET /howto 返回
    pub howto_file: PathBuf,
    /// 首页 footer 里的 GitHub 链接
    pub github_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    pub repos_dir: PathBuf,
    pub worktrees_dir: PathBuf,
    pub default_branch: String,
    pub hook_template: PathBuf,
    /// SQLite 数据库路径（用户表 + session 表）。和 repos_dir/worktrees_dir
    /// 同级目录是惯例；不写则用 repos_dir 父目录下 gitlook.db。
    pub db_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StaticConfig {
    pub auto_index: bool,
    pub index_template: PathBuf,
    /// 首页（GET /）的模板，列出所有已托管的站点
    pub home_template: PathBuf,
    /// 首次设置页面（GET /setup）的模板
    pub setup_template: PathBuf,
    /// 登录页面（GET /login）的模板
    pub login_template: PathBuf,
    /// 管理页面（GET /admin）的模板
    pub admin_template: PathBuf,
    pub spa_fallback: bool,
    pub cache_max_age: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebhookConfig {
    pub enabled: bool,
    pub secret: String,
    pub allowed_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
    pub json_format: bool,
    pub file_output: Option<PathBuf>,
}

// 默认值集中在这里；Config::default() 只是把各段拼起来，
// 避免同一份默认值在两处各写一遍而走偏。
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 9999,
            base_path: "".into(),
            max_body_size: 100 * 1024 * 1024, // 100MB
            request_timeout_secs: 300,
            howto_file: PathBuf::from("docs/HOWTO.md"),
            github_url: "https://github.com/PandaJiuJiu/GitLook".to_string(),
        }
    }
}

impl Default for GitConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let worktrees_dir = home.join("gitlook/worktrees");
        Self {
            repos_dir: home.join("gitlook/repos"),
            worktrees_dir: worktrees_dir.clone(),
            default_branch: "main".into(),
            hook_template: PathBuf::from("hooks/post-receive"),
            db_path: worktrees_dir.parent().unwrap_or(&home).join("gitlook.db"),
        }
    }
}

impl Default for StaticConfig {
    fn default() -> Self {
        Self {
            auto_index: true,
            index_template: PathBuf::from("templates/dir_index.html.tera"),
            home_template: PathBuf::from("templates/home.html.tera"),
            setup_template: PathBuf::from("templates/setup.html.tera"),
            login_template: PathBuf::from("templates/login.html.tera"),
            admin_template: PathBuf::from("templates/admin.html.tera"),
            spa_fallback: false,
            cache_max_age: 3600,
        }
    }
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            secret: "".into(),
            allowed_events: vec!["push".into()],
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".into(),
            json_format: false,
            file_output: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            git: GitConfig::default(),
            static_files: StaticConfig::default(),
            webhook: WebhookConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Config {
    /// 从指定路径加载配置
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();

        // 文件不存在：视为首次运行，使用默认配置
        let toml_str = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    "Config file {} not found, using defaults. \
                     Run with --init-config to generate one.",
                    path.display()
                );
                return Ok(Config::default().expand_paths());
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Cannot read {}: {}", path.display(), e));
            }
        };

        // 文件存在但解析失败：直接报错。
        // 静默回退到默认配置会让服务用错误的目录启动，比启动失败更难排查。
        let config: Config = toml::from_str(&toml_str).map_err(|e| {
            anyhow::anyhow!("Failed to parse {}: {}", path.display(), e)
        })?;

        Ok(config.expand_paths())
    }

    fn expand_paths(self) -> Self {
        let expand = |p: PathBuf| -> PathBuf {
            if p.to_string_lossy().starts_with("~") {
                if let Some(home) = dirs::home_dir() {
                    let stripped = p.strip_prefix("~").unwrap();
                    return home.join(stripped);
                }
            }
            p
        };

        Config {
            server: ServerConfig {
                howto_file: expand(self.server.howto_file),
                github_url: self.server.github_url,
                ..self.server
            },
            git: GitConfig {
                repos_dir: expand(self.git.repos_dir),
                worktrees_dir: expand(self.git.worktrees_dir),
                db_path: expand(self.git.db_path),
                ..self.git
            },
            static_files: StaticConfig {
                index_template: expand(self.static_files.index_template),
                home_template: expand(self.static_files.home_template),
                setup_template: expand(self.static_files.setup_template),
                login_template: expand(self.static_files.login_template),
                admin_template: expand(self.static_files.admin_template),
                ..self.static_files
            },
            logging: LoggingConfig {
                file_output: self.logging.file_output.map(expand),
                ..self.logging
            },
            ..self
        }
    }

    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }

    pub fn base_path(&self) -> &str {
        self.server.base_path.trim_end_matches('/')
    }
}