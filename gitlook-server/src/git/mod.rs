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

/// 求 `from` 目录到 `to` 路径的相对路径。
///
/// 只处理同为绝对或同为相对的路径；相差层数过多时返回 None，
/// 免得产出一长串 `../..` 的荒谬结果。
fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    if from.is_absolute() != to.is_absolute() {
        return None;
    }

    let mut from_parts = from.components();
    let mut to_parts = to.components();

    // 剥掉公共前缀
    loop {
        match (from_parts.clone().next(), to_parts.clone().next()) {
            (Some(a), Some(b)) if a == b => {
                from_parts.next();
                to_parts.next();
            }
            _ => break,
        }
    }

    let ups = from_parts.count();
    if ups > 16 {
        return None;
    }

    let rest: PathBuf = to_parts.collect();
    let mut out = PathBuf::new();
    for _ in 0..ups {
        out.push("..");
    }
    out.push(rest);

    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
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
            let full = String::from_utf8_lossy(&output.stdout).trim().to_string();
            full.strip_prefix("refs/heads/").unwrap_or(&full).to_string()
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

        // 把 worktree 位置记成相对于裸仓库所在目录的相对路径，供 post-receive
        // 钩子在运行时解析（见 DEFAULT_HOOK_TEMPLATE 里的说明）。
        let relpath = relative_path(bare_path.parent().unwrap(), &worktree_path)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Cannot express worktree path {} relative to {}",
                    worktree_path.display(),
                    self.repos_dir.display()
                )
            })?;

        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "config", "gitlook.worktree-relpath"])
            .arg(&relpath)
            .output()
            .context("Failed to execute git config")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::OperationFailed(stderr.to_string()).into());
        }

        // 初始化工作树并创建初始提交
        let output = Command::new("git")
            .args(["--git-dir", bare_path.to_str().unwrap(), "--work-tree", worktree_path.to_str().unwrap()])
            .args(["checkout", "-b", &self.default_branch])
            .output()
            .context("Failed to execute git checkout")?;

        if !output.status.success() {
            // 可能是因为还没有提交，忽略这个错误
        }

        // 创建初始 README.md 并提交
        let readme_path = worktree_path.join("README.md");
        tokio_fs::write(&readme_path, format!("# {}\n\nWelcome to your gitlook repository!\n", name)).await?;

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
# gitlook post-receive hook
# Auto-generated - do not edit directly

REPO_NAME="{{REPO_NAME}}"
BRANCH="{{DEFAULT_BRANCH}}"

# 从脚本自身位置推出 GIT_DIR，不用 $GIT_DIR 环境变量：
# 容器部署时 push 由宿主机发起，钩子在宿主机上跑，$GIT_DIR 会是宿主机路径。
GIT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# worktree 路径存成"相对于裸仓库所在目录"的相对路径，在创建仓库时写入。
# 钩子执行时才知道自己被挂载到了哪里，因此不能把绝对路径写死——
# 宿主机上是 ~/gitlook/worktrees，容器里是 /var/lib/gitlook/worktrees，
# 写死哪个都会在另一边失效。相对路径两边都成立。
REL="$(git --git-dir="$GIT_DIR" config --get gitlook.worktree-relpath)"
if [ -z "$REL" ]; then
    echo "gitlook: 仓库 $REPO_NAME 缺少 gitlook.worktree-relpath 配置，无法部署" >&2
    exit 1
fi
WORKTREE="$(cd "$GIT_DIR/.." && cd "$REL" && pwd)"

if [ ! -d "$WORKTREE" ]; then
    mkdir -p "$WORKTREE" || {
        echo "gitlook: 无法创建 worktree $WORKTREE" >&2
        exit 1
    }
fi

# Read stdin (oldrev newrev refname)
while read oldrev newrev refname; do
    # Only deploy the default branch
    if [ "$refname" = "refs/heads/$BRANCH" ]; then
        echo "Deploying $REPO_NAME to $WORKTREE..."
        if git --git-dir="$GIT_DIR" --work-tree="$WORKTREE" checkout -f "$BRANCH"; then
            echo "Deployment complete for $REPO_NAME"
        else
            echo "gitlook: 部署 $REPO_NAME 失败" >&2
            exit 1
        fi
    fi
done
"#;
#[cfg(test)]
mod tests {
    use super::relative_path;

    #[test]
    fn sibling_directories() {
        // 这是容器部署的实际布局：repos 与 worktrees 是同级目录
        assert_eq!(
            relative_path(
                std::path::Path::new("/var/lib/gitlook/repos"),
                std::path::Path::new("/var/lib/gitlook/worktrees/demo"),
            ),
            Some("../worktrees/demo".into())
        );
    }

    #[test]
    fn sibling_directories_under_home() {
        // 同一个相对路径在宿主机上也成立，这就是不用绝对路径的原因
        assert_eq!(
            relative_path(
                std::path::Path::new("/home/zac/gitlook/repos"),
                std::path::Path::new("/home/zac/gitlook/worktrees/demo"),
            ),
            Some("../worktrees/demo".into())
        );
    }

    #[test]
    fn descends_from_common_ancestor() {
        assert_eq!(
            relative_path(
                std::path::Path::new("/srv/git/repos"),
                std::path::Path::new("/srv/sites/www"),
            ),
            Some("../../sites/www".into())
        );
    }

    #[test]
    fn mixed_absolute_and_relative_is_rejected() {
        assert_eq!(
            relative_path(
                std::path::Path::new("/var/lib/gitlook/repos"),
                std::path::Path::new("worktrees/demo"),
            ),
            None
        );
    }

    #[test]
    fn unreasonably_distant_paths_are_rejected() {
        let deep = "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y";
        assert_eq!(
            relative_path(
                std::path::Path::new(deep),
                std::path::Path::new("/var/lib/gitlook/worktrees/demo"),
            ),
            None
        );
    }
}
