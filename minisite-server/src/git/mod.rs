use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tokio::fs as tokio_fs;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub name: String,
    pub bare_path: PathBuf,
    pub worktree_path: PathBuf,
    pub default_branch: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Repository not found: {0}")]
    NotFound(String),
    #[error("Repository already exists: {0}")]
    AlreadyExists(String),
    #[error("Invalid repository name: {0}")]
    InvalidName(String),
    #[error("Git operation failed: {0}")]
    OperationFailed(String),
    #[error("Hook installation failed: {0}")]
    HookFailed(String),
}

pub struct GitManager {
    repos_dir: PathBuf,
    worktrees_dir: PathBuf,
    default_branch: String,
    hook_template: String,
}

impl GitManager {
    pub async fn new(repos_dir: PathBuf, worktrees_dir: PathBuf, default_branch: String, hook_template: PathBuf) -> Result<Self> {
        // 确保目录存在
        tokio_fs::create_dir_all(&repos_dir).await?;
        tokio_fs::create_dir_all(&worktrees_dir).await?;

        // 读取 hook 模板
        let hook_template = if hook_template.exists() {
            tokio_fs::read_to_string(&hook_template).await?
        } else {
            DEFAULT_HOOK_TEMPLATE.to_string()
        };

        Ok(Self {
            repos_dir,
            worktrees_dir,
            default_branch,
            hook_template,
        })
    }

    /// 列出所有仓库
    pub async fn list_repos(&self) -> Result<Vec<RepoInfo>> {
        let mut repos = Vec::new();
        let mut entries = tokio_fs::read_dir(&self.repos_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".git") {
                let repo_name = name.strip_suffix(".git").unwrap().to_string();
                let bare_path = entry.path();
                let worktree_path = self.worktrees_dir.join(&repo_name);

                let meta = self.get_repo_metadata(&bare_path, &repo_name).await?;
                repos.push(meta);
            }
        }

        repos.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(repos)
    }

    async fn get_repo_metadata(&self, bare_path: &Path, name: &str) -> Result<RepoInfo> {
        // 使用 git 命令行获取默认分支
        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap()])
            .args(["symbolic-ref", "HEAD"])
            .output()
            .context("Failed to execute git symbolic-ref")?;

        let default_branch = if output.status.success() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            self.default_branch.clone()
        };

        let meta = tokio_fs::metadata(bare_path).await?;
        let created_at = meta.created().ok()
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t))
            .unwrap_or_else(chrono::Utc::now);

        let updated_at = meta.modified().ok()
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t));

        Ok(RepoInfo {
            name: name.to_string(),
            bare_path: bare_path.to_path_buf(),
            worktree_path: self.worktrees_dir.join(name),
            default_branch,
            created_at,
            updated_at,
        })
    }

    /// 创建新仓库
    pub async fn create_repo(&self, name: &str) -> Result<RepoInfo> {
        Self::validate_name(name)?;

        let bare_path = self.repos_dir.join(format!("{}.git", name));
        let worktree_path = self.worktrees_dir.join(name);

        if bare_path.exists() {
            return Err(GitError::AlreadyExists(name.to_string()).into());
        }

        // 使用 git 命令行创建裸仓库（避免 git2 的 Send/Sync 问题）
        let output = Command::new("git")
            .args(["init", "--bare", bare_path.to_str().unwrap()])
            .output()
            .context("Failed to execute git init")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::OperationFailed(stderr.to_string()).into());
        }

        // 创建工作树目录
        tokio_fs::create_dir_all(&worktree_path).await?;

        // 初始化工作树并创建初始提交
        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "--work-tree", worktree_path.to_str().unwrap()])
            .args(["checkout", "-b", &self.default_branch])
            .output()
            .context("Failed to execute git checkout")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // 可能是因为还没有提交，忽略这个错误
        }

        // 创建初始 README.md 并提交
        let readme_path = worktree_path.join("README.md");
        tokio_fs::write(&readme_path, format!("# {}\n\nWelcome to your minisite repository!\n", name)).await?;

        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "--work-tree", worktree_path.to_str().unwrap()])
            .args(["add", "README.md"])
            .output()
            .context("Failed to execute git add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("git add failed: {}", stderr);
        }

        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "--work-tree", worktree_path.to_str().unwrap()])
            .args(["commit", "-m", "Initial commit"])
            .output()
            .context("Failed to execute git commit")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("git commit failed: {}", stderr);
        }

        // 安装 post-receive hook
        self.install_hook(&bare_path, name).await?;

        info!("Created repository: {}", name);

        Ok(RepoInfo {
            name: name.to_string(),
            bare_path,
            worktree_path,
            default_branch: self.default_branch.clone(),
            created_at: chrono::Utc::now(),
            updated_at: None,
        })
    }

    /// 删除仓库
    pub async fn delete_repo(&self, name: &str) -> Result<()> {
        let bare_path = self.repos_dir.join(format!("{}.git", name));
        let worktree_path = self.worktrees_dir.join(name);

        if !bare_path.exists() {
            return Err(GitError::NotFound(name.to_string()).into());
        }

        tokio_fs::remove_dir_all(&bare_path).await?;
        if worktree_path.exists() {
            tokio_fs::remove_dir_all(&worktree_path).await?;
        }

        info!("Deleted repository: {}", name);
        Ok(())
    }

    /// 触发部署（更新工作树）
    pub async fn deploy(&self, name: &str) -> Result<DeployResult> {
        let bare_path = self.repos_dir.join(format!("{}.git", name));
        let worktree_path = self.worktrees_dir.join(name);

        if !bare_path.exists() {
            return Err(GitError::NotFound(name.to_string()).into());
        }

        // 使用 git 命令行更新工作树（更可靠）
        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "--work-tree", worktree_path.to_str().unwrap()])
            .args(["checkout", "-f", &self.default_branch])
            .output()
            .context("Failed to execute git checkout")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Git checkout failed: {}", stderr);
            return Err(GitError::OperationFailed(stderr.to_string()).into());
        }

        // 使用 git 命令行获取最新 commit hash
        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap()])
            .args(["rev-parse", "HEAD"])
            .output()
            .context("Failed to execute git rev-parse")?;

        let commit_hash = if output.status.success() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            String::new()
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        info!("Deployed {}: {}", name, stdout.trim());

        Ok(DeployResult {
            repo_name: name.to_string(),
            success: true,
            message: stdout.trim().to_string(),
            commit_hash,
        })
    }

    async fn install_hook(&self, bare_path: &Path, repo_name: &str) -> Result<()> {
        let hooks_dir = bare_path.join("hooks");
        tokio_fs::create_dir_all(&hooks_dir).await?;

        let hook_path = hooks_dir.join("post-receive");
        let hook_content = self.render_hook(repo_name);

        tokio_fs::write(&hook_path, hook_content).await?;

        // 设置可执行权限
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = tokio_fs::metadata(&hook_path).await?.permissions();
            perms.set_mode(0o755);
            tokio_fs::set_permissions(&hook_path, perms).await?;
        }

        debug!("Installed post-receive hook for {}", repo_name);
        Ok(())
    }

    fn render_hook(&self, repo_name: &str) -> String {
        self.hook_template
            .replace("{{REPO_NAME}}", repo_name)
            .replace("{{WORKTREES_DIR}}", self.worktrees_dir.to_str().unwrap())
            .replace("{{DEFAULT_BRANCH}}", &self.default_branch)
    }

    async fn ensure_initial_commit(&self, _bare_path: &Path, _worktree_path: &Path) -> Result<()> {
        // 此函数已被废弃 - 初始提交在 create_repo 中直接处理
        Ok(())
    }

    fn validate_name(name: &str) -> Result<()> {
        if name.is_empty() || name.len() > 100 {
            return Err(GitError::InvalidName("Name must be 1-100 characters".into()).into());
        }
        if name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '.']) {
            return Err(GitError::InvalidName("Name contains invalid characters".into()).into());
        }
        if name.starts_with('.') || name.starts_with('-') {
            return Err(GitError::InvalidName("Name cannot start with . or -".into()).into());
        }
        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
pub struct DeployResult {
    pub repo_name: String,
    pub success: bool,
    pub message: String,
    pub commit_hash: String,
}

const DEFAULT_HOOK_TEMPLATE: &str = r#"#!/bin/bash
# minisite post-receive hook
# Auto-generated - do not edit directly

REPO_NAME="{{REPO_NAME}}"
GIT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
WORKTREE="{{WORKTREES_DIR}}/{{REPO_NAME}}"
BRANCH="{{DEFAULT_BRANCH}}"

# Read stdin (oldrev newrev refname)
while read oldrev newrev refname; do
    # Only deploy the default branch
    if [ "$refname" = "refs/heads/$BRANCH" ]; then
        echo "Deploying $REPO_NAME to $WORKTREE..."
        git --git-dir="$GIT_DIR" --work-tree="$WORKTREE" checkout -f "$BRANCH"
        echo "Deployment complete for $REPO_NAME"
    fi
done
"#;