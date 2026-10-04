use anyhow::Result;
use clap::{Parser, Subcommand};
use minisite_server::{config::Config, git::GitManager};

#[derive(Parser, Debug)]
#[command(name = "minisite-cli", version, about = "CLI for managing minisite repositories")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Config file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List all repositories
    List,
    /// Create a new repository
    Create {
        /// Repository name
        name: String,
    },
    /// Delete a repository
    Delete {
        /// Repository name
        name: String,
    },
    /// Deploy a repository (update worktree)
    Deploy {
        /// Repository name
        name: String,
    },
    /// Show repository info
    Info {
        /// Repository name
        name: String,
    },
    /// Initialize configuration
    Init {
        /// Config file path
        #[arg(short, long, default_value = "config.toml")]
        output: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;

    let git = GitManager::new(
        config.git.repos_dir,
        config.git.worktrees_dir,
        config.git.default_branch,
        config.git.hook_template,
    ).await?;

    match cli.command {
        Commands::List => list_repos(&git).await,
        Commands::Create { name } => create_repo(&git, &name).await,
        Commands::Delete { name } => delete_repo(&git, &name).await,
        Commands::Deploy { name } => deploy_repo(&git, &name).await,
        Commands::Info { name } => info_repo(&git, &name).await,
        Commands::Init { output } => init_config(&output).await,
    }
}

async fn list_repos(git: &GitManager) -> Result<()> {
    let repos = git.list_repos().await?;
    if repos.is_empty() {
        println!("No repositories found.");
        return Ok(());
    }

    println!("{:<30} {:<15} {:<25} {}", "NAME", "BRANCH", "CREATED", "UPDATED");
    println!("{}", "-".repeat(90));
    for repo in repos {
        let updated = repo.updated_at
            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("{:<30} {:<15} {:<25} {}", repo.name, repo.default_branch, repo.created_at.format("%Y-%m-%d %H:%M"), updated);
    }
    Ok(())
}

async fn create_repo(git: &GitManager, name: &str) -> Result<()> {
    let repo = git.create_repo(name).await?;
    println!("✅ Repository '{}' created successfully!", name);
    println!("   Path: {}", repo.bare_path.display());
    println!("   Worktree: {}", repo.worktree_path.display());
    println!("   Default branch: {}", repo.default_branch);
    println!("\nAdd remote and push:");
    println!("   git remote add minisite ssh://user@host/{}.git", name);
    println!("   git push minisite main");
    Ok(())
}

async fn delete_repo(git: &GitManager, name: &str) -> Result<()> {
    git.delete_repo(name).await?;
    println!("✅ Repository '{}' deleted.", name);
    Ok(())
}

async fn deploy_repo(git: &GitManager, name: &str) -> Result<()> {
    let result = git.deploy(name).await?;
    if result.success {
        println!("✅ Deployed '{}' successfully!", name);
        println!("   Commit: {}", result.commit_hash);
        println!("   Message: {}", result.message);
    } else {
        println!("❌ Deployment failed: {}", result.message);
    }
    Ok(())
}

async fn info_repo(git: &GitManager, name: &str) -> Result<()> {
    let repos = git.list_repos().await?;
    if let Some(repo) = repos.into_iter().find(|r| r.name == name) {
        println!("Repository: {}", repo.name);
        println!("  Bare path: {}", repo.bare_path.display());
        println!("  Worktree: {}", repo.worktree_path.display());
        println!("  Default branch: {}", repo.default_branch);
        println!("  Created: {}", repo.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
        if let Some(updated) = repo.updated_at {
            println!("  Updated: {}", updated.format("%Y-%m-%d %H:%M:%S UTC"));
        }
    } else {
        println!("Repository '{}' not found.", name);
    }
    Ok(())
}

async fn init_config(output: &str) -> Result<()> {
    let config = Config::default();
    let toml = toml::to_string_pretty(&config)?;
    let commented = format!(
        "# Minisite Configuration\n# Generated at {}\n\n{}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        toml
    );
    tokio::fs::write(output, commented).await?;
    println!("Configuration file created at: {}", output);
    Ok(())
}