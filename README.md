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

The service listens on port **9999** on all interfaces. Sites live in `~/minisite/`
on the host, so the container can be deleted and rebuilt without losing
anything.

Set a token in `contrib/.env` before starting — compose refuses to start
without one:

```bash
cd contrib
echo "MINISITE_API_TOKEN=$(openssl rand -hex 32)" >> .env
docker-compose up -d --build
```

From another machine on the LAN:

```bash
curl http://192.168.1.160:9999/            # service info
curl http://192.168.1.160:9999/health      # -> ok
curl http://192.168.1.160:9999/howto       # Markdown docs for AI agents
open http://192.168.1.160:9999/my-site/    # a hosted site
```

Deploying a site (the API call needs the token; `git push` does not):

```bash
export MINISITE_API_TOKEN='...'            # from contrib/.env

curl http://192.168.1.160:9999/api/repos -X POST \
     -H "Authorization: Bearer $MINISITE_API_TOKEN" \
     -d '{"name":"my-site"}'

git remote add minisite ~/minisite/repos/my-site.git
git push minisite main --force
```

To listen on loopback only — this machine alone, nothing from the LAN —
change the `ports:` line in `contrib/compose.yml` to `127.0.0.1:9999:9999`.

### Authentication

Hosted sites are public; the management API is not. `/api/*` requires
`Authorization: Bearer <token>`, compared in constant time. `git push` is a
separate path and needs no token.

The token is read from `MINISITE_API_TOKEN` in the environment, falling back to
`server.api_token` in `config.toml`. It goes in `contrib/.env` (gitignored)
rather than in a config file, because config files get committed and baked into
images. If neither is set the API is wide open and the server logs a warning at
startup.

### Exposing it to the network

With a token set, publishing on the LAN is reasonable — the API that can delete
your sites is locked. Two things to keep in mind: there is still **no TLS**, so
the token crosses the network in plaintext and can be replayed by anyone who
captures it. And the token is the only thing standing between a LAN guest and
`DELETE /api/repos/{name}`. For anything beyond a trusted network, put TLS in
front first.

If `ufw` is active, open the port or nothing will reach it:

```bash
sudo ufw allow 9999/tcp
sudo ufw status
```

If it is already reachable and you still cannot connect, check whether the
client is on the same subnet (`192.168.1.0/24`) — podman's port forwarder
publishes on all interfaces, but a router set to client isolation will block
same-LAN traffic.

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
