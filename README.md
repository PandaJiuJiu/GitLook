# Minisite

Lightweight self-hosted static site hosting with Git push deployment. Push HTML to a single endpoint, serve it at `http://host:port/repo-name/`.

## Features

- **Git push deployment** — `git push` triggers automatic deployment via post-receive hook
- **Multi-repo under one domain** — each repo accessible at `/repo-name/`
- **Auto directory indexing** with breadcrumb navigation
- **Bare repo + worktree** architecture for clean deploys
- **HTTP REST API** for managing repositories programmatically
- **CLI** for repo management from terminal
- **Pure Rust** implementation using axum

## Quick Start

### 1. Build

```bash
cargo build --release
```

Binaries land in `target/release/`:
- `minisite-server` — HTTP server
- `minisite-cli` — management CLI

### 2. Configure

```bash
./target/release/minisite-cli init --output config.toml
# Edit config.toml
```

Key config values:

```toml
[server]
host = "0.0.0.0"
port = 9999

[git]
repos_dir = "/var/lib/minisite/repos"
worktrees_dir = "/var/lib/minisite/worktrees"
default_branch = "main"
hook_template = "/etc/minisite/hooks/post-receive"
```

### 3. Start the server

```bash
./target/release/minisite-server --config config.toml
```

### 4. Create a repository

```bash
./target/release/minisite-cli create my-site
```

### 5. Push content

```bash
cd ~/my-site-content
git init
echo "<h1>Hello</h1>" > index.html
git add . && git commit -m "Initial"
git remote add minisite /var/lib/minisite/repos/my-site.git
git push minisite main
```

The site is live at `http://localhost:9999/my-site/`.

## Usage

### CLI

```bash
minisite-cli list                # List all repos
minisite-cli create <name>       # Create a repo
minisite-cli delete <name>       # Delete a repo
minisite-cli deploy <name>       # Force-deploy a repo
minisite-cli info <name>         # Show repo details
minisite-cli init                # Generate config.toml
```

### HTTP API

```
GET    /api/repos                 # List repos
POST   /api/repos                 # Create repo {"name":"..."}
DELETE /api/repos/:name           # Delete repo
POST   /api/repos/:name/deploy    # Force deploy
GET    /health                    # Health check
```

Example:

```bash
curl -X POST http://localhost:9999/api/repos \
  -H "Content-Type: application/json" \
  -d '{"name":"my-site"}'
```

### Static file serving

```
GET /repo-name/                   # Directory index
GET /repo-name/path/to/file       # Static file
GET /repo-name/path/to/index.html # Index file
```

Path traversal is prevented — requests cannot escape the repo's worktree directory.

## Architecture

```
client push ──► bare repo (repos/) ──post-receive hook──► worktree (worktrees/)
                                                              │
                                                              ▼
                                                     HTTP serve /repo-name/
```

- **Bare repo** (`repos/<name>.git`) — receives pushes, has post-receive hook installed
- **Worktree** (`worktrees/<name>/`) — checked-out files served by HTTP
- **post-receive hook** — runs `git checkout -f main` against the worktree on push

## Deployment

### systemd

Copy `contrib/systemd/minisite.service` to `/etc/systemd/system/` and edit paths:

```bash
sudo cp contrib/systemd/minisite.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now minisite
```

### Docker

```bash
docker build -f contrib/docker/Dockerfile -t minisite .
docker run -d -p 9999:9999 \
  -v /var/lib/minisite:/var/lib/minisite \
  -v /etc/minisite:/etc/minisite \
  minisite
```

## Development

```bash
cargo run --bin minisite-server -- --config config.toml
cargo test
```

## License

MIT
