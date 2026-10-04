# Minisite

Lightweight self-hosted static site hosting with Git push deployment.

## Features

- 🚀 **Git push to deploy** - Just `git push` and your site updates instantly
- 📁 **Multiple repositories** - Host many sites under one domain: `http://host:9999/repo-name/`
- 🔍 **Auto directory indexing** - Beautiful directory listings with breadcrumbs
- ⚡ **Zero config** - Single binary, minimal dependencies
- 🔒 **Secure** - Path traversal protection, proper MIME types, caching headers
- 📦 **Pure Rust** - Built with `gix` (pure Rust Git), no C dependencies

## Quick Start

### 1. Build

```bash
cargo build --release --workspace
```

### 2. Initialize config

```bash
./target/release/minisite --init-config
# Edit config.toml as needed
```

### 3. Run server

```bash
./target/release/minisite
# Server starts on http://0.0.0.0:9999
```

### 4. Create a repository

```bash
# Using CLI
./target/release/minisite-cli create my-docs

# Or via API
curl -X POST http://localhost:9999/api/repos -H "Content-Type: application/json" -d '{"name": "my-docs"}'
```

### 5. Push your site

```bash
cd my-site
git init
git remote add minisite ssh://user@your-server:9999/~/minisite/repos/my-docs.git
# Or HTTPS if configured
git remote add minisite https://your-server/api/repos/my-docs

git add .
git commit -m "Initial commit"
git push minisite main
```

### 6. Access your site

```
http://your-server:9999/my-docs/
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    minisite (Rust)                           │
├─────────────────────────────────────────────────────────────┤
│  HTTP Server (axum)                                          │
│  ├── GET  /:repo/           → Static file serving            │
│  ├── GET  /:repo/:path      → Files with directory index     │
│  ├── GET  /api/repos        → List repositories              │
│  ├── POST /api/repos        → Create repository              │
│  ├── DELETE /api/repos/:n   → Delete repository              │
│  └── POST /api/repos/:n/deploy → Manual deploy trigger       │
├─────────────────────────────────────────────────────────────┤
│  Git Manager (gix)                                           │
│  ├── ~/repos/:repo.git/      ← Bare repo (receives push)     │
│  │   └── hooks/post-receive  → Auto checkout to worktree     │
│  └── ~/worktrees/:repo/      ← Worktree (served via HTTP)    │
└─────────────────────────────────────────────────────────────┘
```

## Configuration

See `config.toml` for all options:

```toml
[server]
host = "0.0.0.0"
port = 9999
base_path = ""          # Base path prefix, e.g. "/sites"

[git]
repos_dir = "~/minisite/repos"
worktrees_dir = "~/minisite/worktrees"
default_branch = "main"

[static_files]
auto_index = true       # Enable directory listings
spa_fallback = false    # Fallback to index.html for SPA
cache_max_age = 3600    # Cache-Control max-age
```

## Deployment

### Systemd Service

```ini
# /etc/systemd/system/minisite.service
[Unit]
Description=Minisite Static Site Hosting
After=network.target

[Service]
Type=simple
User=minisite
WorkingDirectory=/opt/minisite
ExecStart=/opt/minisite/minisite
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

### Docker

```dockerfile
FROM rust:1.78 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --workspace

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates git && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/minisite /usr/local/bin/
COPY --from=builder /app/target/release/minisite-cli /usr/local/bin/
COPY config.toml /etc/minisite/config.toml
COPY templates/ /etc/minisite/templates/
COPY hooks/ /etc/minisite/hooks/
WORKDIR /var/lib/minisite
EXPOSE 9999
CMD ["minisite"]
```

## CLI Usage

```bash
# List repositories
minisite-cli list

# Create repository
minisite-cli create my-site

# Delete repository
minisite-cli delete my-site

# Manual deploy
minisite-cli deploy my-site

# Show info
minisite-cli info my-site
```

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/repos` | List all repositories |
| POST | `/api/repos` | Create new repository |
| DELETE | `/api/repos/:name` | Delete repository |
| POST | `/api/repos/:name/deploy` | Trigger manual deploy |
| GET | `/:repo/` | Serve repository root |
| GET | `/:repo/:path` | Serve file or directory index |
| GET | `/health` | Health check |

## License

MIT