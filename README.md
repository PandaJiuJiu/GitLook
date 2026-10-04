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

Docker is the deployment method. `docker-compose` handles building, running,
and restart-on-boot:

```bash
cd contrib
docker-compose up -d --build     # build and start
docker-compose logs -f           # follow logs
docker-compose restart           # restart (needed after editing config)
docker-compose down              # stop
```

The service listens on **127.0.0.1:9999 only**. Sites live in `~/minisite/` on
the host, so the container can be deleted and rebuilt without losing anything.

```bash
curl http://127.0.0.1:9999/api/repos -X POST -d '{"name":"my-site"}'
git remote add minisite ~/minisite/repos/my-site.git
git push minisite main --force
curl http://127.0.0.1:9999/my-site/
```

`curl http://127.0.0.1:9999/howto` returns a Markdown document describing the
whole workflow — useful for handing to an AI agent.

### Exposing it to the network

**There is no authentication.** Anyone who can reach the port can create
repositories, or delete yours with `DELETE /api/repos/{name}`. To publish on
the LAN, change the `ports:` line in `contrib/compose.yml` from
`127.0.0.1:9999:9999` to `9999:9999` — but understand what you are opening up.

### Running under podman

If `docker` is podman rather than Docker, two things need attention:

- **Socket.** Point compose at the user socket:
  `export DOCKER_HOST=unix:///run/user/$UID/podman/podman.sock`
- **Network.** `compose.yml` reuses podman's built-in `podman` network as an
  external network, because compose-created networks get CNI configVersion
  1.0.0 that podman's bundled plugins reject. On real Docker, delete that
  `networks:` block.
- **Restart can fail spuriously.** `docker-compose restart` sometimes reports
  `bind: address already in use` — rootless podman's port forwarder takes a
  moment to release the port. Wait a few seconds and run `docker-compose up -d`
  again. Data is unaffected; nothing is lost when this happens.

If Docker Hub is unreachable, put the registry prefix in `contrib/.env`:

```
MINISITE_REGISTRY=public.ecr.aws/docker/library
```

## Development

```bash
cargo run --bin minisite-server -- --config config.toml
cargo test
```

## License

MIT
