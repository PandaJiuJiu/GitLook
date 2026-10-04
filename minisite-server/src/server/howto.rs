//! HowTo 端点：返回结构化的使用说明，AI/脚本可一次 curl 拿到完整操作步骤

use axum::{http::header, response::Response, Json};
use serde::Serialize;

/// HowTo 响应：结构化 + 人类可读
#[derive(Serialize)]
pub struct HowTo {
    pub service: ServiceInfo,
    pub quick_start: Vec<Step>,
    pub api: Vec<ApiEndpoint>,
    pub cli: Vec<CliCommand>,
    pub push_workflow: PushWorkflow,
    pub gotchas: Vec<String>,
}

#[derive(Serialize)]
pub struct ServiceInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub version: &'static str,
    pub base_url: &'static str,
    pub entry_point: &'static str,
    pub routes: Routes,
}

#[derive(Serialize)]
pub struct Routes {
    pub repo_pattern: &'static str,
    pub examples: Vec<&'static str>,
}

#[derive(Serialize)]
pub struct Step {
    pub order: u32,
    pub title: &'static str,
    pub description: &'static str,
    pub command: &'static str,
    pub expects: &'static str,
}

#[derive(Serialize)]
pub struct ApiEndpoint {
    pub method: &'static str,
    pub path: &'static str,
    pub purpose: &'static str,
    pub body_example: Option<&'static str>,
    pub curl_example: &'static str,
}

#[derive(Serialize)]
pub struct CliCommand {
    pub command: &'static str,
    pub purpose: &'static str,
    pub example: &'static str,
}

#[derive(Serialize)]
pub struct PushWorkflow {
    pub overview: &'static str,
    pub steps: Vec<PushStep>,
}

#[derive(Serialize)]
pub struct PushStep {
    pub phase: &'static str,
    pub client_action: &'static str,
    pub server_action: &'static str,
}

/// 返回 Markdown 格式的 HowTo（默认）
pub async fn howto_markdown() -> Response {
    let md = include_str!("../../../docs/HOWTO.md");
    Response::builder()
        .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
        .body(axum::body::Body::from(md.to_string()))
        .unwrap()
}

/// 返回 JSON 格式的 HowTo（结构化，便于程序化消费）
pub async fn howto_json() -> Json<HowTo> {
    Json(HowTo {
        service: ServiceInfo {
            name: "minisite",
            description: "Lightweight self-hosted static site hosting with Git push deployment. Push HTML/JS/CSS files via git, serve them at a single URL under path-based multi-repo routing.",
            version: env!("CARGO_PKG_VERSION"),
            base_url: "http://<host>:<port>",
            entry_point: "/",
            routes: Routes {
                repo_pattern: "/{repo-name}/",
                examples: vec![
                    "GET /                              -> API listing or root info",
                    "GET /api/repos                     -> list all repos",
                    "POST /api/repos                     -> create repo",
                    "GET /{repo}/                       -> directory index for that repo's worktree",
                    "GET /{repo}/path/to/file.html      -> serve a file from that repo",
                    "GET /howto                         -> this page (Markdown)",
                    "GET /howto.json                    -> this page (JSON)",
                    "GET /health                        -> health check",
                ],
            },
        },

        quick_start: vec![
            Step {
                order: 1,
                title: "Discover this server",
                description: "Find this HowTo page first",
                command: "curl http://<host>:<port>/howto",
                expects: "Markdown instructions (this document)",
            },
            Step {
                order: 2,
                title: "Check server health",
                description: "Verify the server is reachable",
                command: "curl http://<host>:<port>/health",
                expects: "ok",
            },
            Step {
                order: 3,
                title: "List existing repositories",
                description: "See what repos already exist",
                command: "curl http://<host>:<port>/api/repos",
                expects: "JSON with `data.repos` array",
            },
            Step {
                order: 4,
                title: "Create a new repository",
                description: "Allocate a bare repo + worktree for your site",
                command: "curl -X POST http://<host>:<port>/api/repos -H 'Content-Type: application/json' -d '{\"name\":\"my-site\"}'",
                expects: "JSON with `data.name` = 'my-site'",
            },
            Step {
                order: 5,
                title: "Push content to deploy",
                description: "Standard git push; post-receive hook auto-deploys",
                command: "git push minisite main",
                expects: "remote output 'Deployment complete for my-site'",
            },
            Step {
                order: 6,
                title: "Access the site",
                description: "Open the URL in a browser or curl it",
                command: "curl http://<host>:<port>/my-site/",
                expects: "HTML directory index (or index.html if present)",
            },
        ],

        api: vec![
            ApiEndpoint {
                method: "GET",
                path: "/api/repos",
                purpose: "List all repositories with metadata (name, branch, timestamps)",
                body_example: None,
                curl_example: "curl http://<host>:<port>/api/repos",
            },
            ApiEndpoint {
                method: "POST",
                path: "/api/repos",
                purpose: "Create a new repository. Returns 409 if name exists, 400 if invalid.",
                body_example: Some(r#"{"name":"my-site"}"#),
                curl_example: "curl -X POST http://<host>:<port>/api/repos -H 'Content-Type: application/json' -d '{\"name\":\"my-site\"}'",
            },
            ApiEndpoint {
                method: "DELETE",
                path: "/api/repos/{name}",
                purpose: "Delete a repository and its worktree",
                body_example: None,
                curl_example: "curl -X DELETE http://<host>:<port>/api/repos/my-site",
            },
            ApiEndpoint {
                method: "POST",
                path: "/api/repos/{name}/deploy",
                purpose: "Force-deploy: re-checkout default branch into worktree",
                body_example: None,
                curl_example: "curl -X POST http://<host>:<port>/api/repos/my-site/deploy",
            },
            ApiEndpoint {
                method: "GET",
                path: "/{repo}/",
                purpose: "Browse the deployed worktree (directory index)",
                body_example: None,
                curl_example: "curl http://<host>:<port>/my-site/",
            },
            ApiEndpoint {
                method: "GET",
                path: "/{repo}/{path}",
                purpose: "Serve a file from the repo's worktree",
                body_example: None,
                curl_example: "curl http://<host>:<port>/my-site/index.html",
            },
        ],

        cli: vec![
            CliCommand {
                command: "list",
                purpose: "List all repositories (table format)",
                example: "minisite-cli list",
            },
            CliCommand {
                command: "create <name>",
                purpose: "Create a new repository",
                example: "minisite-cli create my-site",
            },
            CliCommand {
                command: "delete <name>",
                purpose: "Delete a repository",
                example: "minisite-cli delete my-site",
            },
            CliCommand {
                command: "deploy <name>",
                purpose: "Manually trigger deployment",
                example: "minisite-cli deploy my-site",
            },
            CliCommand {
                command: "info <name>",
                purpose: "Show details for one repository",
                example: "minisite-cli info my-site",
            },
            CliCommand {
                command: "init",
                purpose: "Generate a starter config.toml",
                example: "minisite-cli init --output config.toml",
            },
        ],

        push_workflow: PushWorkflow {
            overview: "Push your HTML/JS/CSS to the server's bare repo. A post-receive hook (auto-installed) runs `git checkout -f main` against the worktree, making new files immediately visible at the HTTP entry point.",
            steps: vec![
                PushStep {
                    phase: "prepare",
                    client_action: "Build your static site locally (HTML, CSS, JS, images). Optional: create index.html as the entry.",
                    server_action: "Server exposes bare repo at repos/<name>.git/ and serves worktree at worktrees/<name>/",
                },
                PushStep {
                    phase: "init",
                    client_action: "In your site directory: `git init -b main && git add . && git commit -m 'init'`",
                    server_action: "Wait for first push",
                },
                PushStep {
                    phase: "remote",
                    client_action: "`git remote add minisite <bare-repo-path>` where bare-repo-path is from the create response (e.g., /var/lib/minisite/repos/my-site.git or ssh://user@host/repos/my-site.git if you wrap with sshd)",
                    server_action: "n/a",
                },
                PushStep {
                    phase: "push",
                    client_action: "`git push minisite main --force` (force may be needed if server's initial commit diverges from your first push)",
                    server_action: "post-receive hook fires: `git --git-dir=<bare> --work-tree=<worktree> checkout -f main`. Worktree is updated atomically.",
                },
                PushStep {
                    phase: "verify",
                    client_action: "Open http://<host>:<port>/my-site/ in browser or curl it",
                    server_action: "StaticFileServer reads from worktree and returns file or directory index",
                },
                PushStep {
                    phase: "update",
                    client_action: "Repeat: edit files, `git add . && git commit -m 'update' && git push minisite main`",
                    server_action: "Each push redeploys. No build step. No restart needed.",
                },
            ],
        },

        gotchas: vec![
            "Repository names: 1-100 chars, only [a-zA-Z0-9_-]. Cannot start with . or -, no slashes.".to_string(),
            "First push often needs `--force` because the server's initial empty commit creates a ref your local doesn't have.".to_string(),
            "The post-receive hook only deploys the configured `default_branch` (default: `main`). Pushes to other branches are accepted but ignored for deployment.".to_string(),
            "Files served are exactly what's in the worktree. No build, no transform, no cache invalidation needed.".to_string(),
            "Path traversal is blocked: /my-site/../blog/ returns 404.".to_string(),
            "If the repo doesn't exist, /{repo}/... returns 404 (not 403), so it can't be used to enumerate names.".to_string(),
            "Directory listing is auto-generated unless `index.html` exists in that directory, in which case the index is served instead.".to_string(),
        ],
    })
}

/// 返回纯文本格式（最简单，AI 直接读）
pub async fn howto_plain() -> Response {
    let text = r#"minisite — self-hosted static site hosting via git push

SERVICE
  Entry point: GET /
  Health:      GET /health  -> "ok"
  HowTo:       GET /howto (markdown), GET /howto.json (json), GET /howto.txt (this)

API
  GET    /api/repos                       list repos
  POST   /api/repos   body={"name":"x"}   create repo x
  DELETE /api/repos/{name}                delete repo
  POST   /api/repos/{name}/deploy         force re-deploy

STATIC
  GET /{repo}/                            directory index (or auto-generated listing)
  GET /{repo}/path/to/file                serve file from worktree

FULL PUSH WORKFLOW (one repo, name=my-site)

  1. Create the repo:
     curl -X POST http://HOST:PORT/api/repos \
          -H 'Content-Type: application/json' \
          -d '{"name":"my-site"}'

  2. Locally, build your site:
     mkdir ~/my-site && cd ~/my-site
     echo '<h1>Hello</h1>' > index.html
     git init -b main
     git add . && git commit -m init

  3. Add minisite as remote:
     # bare-repo-path is whatever the server stores under repos/, e.g.:
     git remote add minisite /var/lib/minisite/repos/my-site.git
     # (or ssh://user@host/path/to/repos/my-site.git if SSH access is set up)

  4. Push (force often needed for the very first push):
     git push minisite main --force

  5. Verify:
     curl http://HOST:PORT/my-site/

NOTES
  - Repo names: alphanumeric + _-, 1-100 chars, no leading dot/dash.
  - post-receive hook auto-installs and only deploys the default branch.
  - No build step. Files in the worktree are served as-is.
  - Path traversal is blocked.
"#;
    Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(axum::body::Body::from(text.to_string()))
        .unwrap()
}
