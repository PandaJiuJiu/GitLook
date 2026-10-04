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
    /// 保护 /api/* 的 Bearer token。为空则完全不鉴权（仅适合只监听回环）。
    /// 留空时从环境变量 MINISITE_API_TOKEN 读取，这样真值不必进配置文件。
    pub api_token: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StaticConfig {
    pub auto_index: bool,
    pub index_template: PathBuf,
    /// 首页（GET /）的模板，列出所有已托管的站点
    pub home_template: PathBuf,
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
            api_token: None,
            github_url: "https://github.com/PandaJiuJiu/GitLook".to_string(),
        }
    }
}

impl Default for GitConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            repos_dir: home.join("gitlook/repos"),
            worktrees_dir: home.join("gitlook/worktrees"),
            default_branch: "main".into(),
            hook_template: PathBuf::from("hooks/post-receive"),
        }
    }
}

impl Default for StaticConfig {
    fn default() -> Self {
        Self {
            auto_index: true,
            index_template: PathBuf::from("templates/dir_index.html.tera"),
            home_template: PathBuf::from("templates/home.html.tera"),
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
                let mut config = Config::default();
                config.resolve_api_token();
                return Ok(config.expand_paths());
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Cannot read {}: {}", path.display(), e));
            }
        };

        // 文件存在但解析失败：直接报错。
        // 静默回退到默认配置会让服务用错误的目录启动，比启动失败更难排查。
        let mut config: Config = toml::from_str(&toml_str).map_err(|e| {
            anyhow::anyhow!("Failed to parse {}: {}", path.display(), e)
        })?;

        config.resolve_api_token();
        Ok(config.expand_paths())
    }

    /// 配置文件里没写 api_token 时，读环境变量 MINISITE_API_TOKEN。
    ///
    /// 走环境变量是为了让真值不必落进配置文件——容器部署时 compose 会把
    /// 文件挂进镜像，配置文件里写 token 等于把它烤进镜像层。
    fn resolve_api_token(&mut self) {
        if self.server.api_token.is_none() {
            self.server.api_token = std::env::var("MINISITE_API_TOKEN")
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
        }
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
                ..self.git
            },
            static_files: StaticConfig {
                index_template: expand(self.static_files.index_template),
                home_template: expand(self.static_files.home_template),
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