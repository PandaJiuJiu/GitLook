# GitLook

Lightweight self-hosted static site hosting with Git push deployment. Push HTML to a Git repository, and it automatically serves at `http://host:port/repo-name/`.

## Features

- **Git push deployment** — push to deploy, no build step
- **Single endpoint** — multiple sites under one domain, each at `/site-name/`
- **Auto directory listing** with breadcrumbs
- **Management API** with token authentication
- **Pure Rust** using axum

## Quick Start

```bash
# Clone and enter the project
git clone git@github.com:PandaJiuJiu/GitLook.git
cd GitLook/contrib

# Generate a secure token (or use your own)
echo "MINISITE_API_TOKEN=$(openssl rand -hex 32)" >> .env

# Start the service
docker-compose up -d --build
```

The service runs on port **9999**. Sites are stored in `~/minisite/` on the host.

## Usage

### Deploy a site

```bash
# 1. Create a repository via API (needs token)
curl -X POST http://localhost:9999/api/repos \
     -H "Authorization: Bearer $MINISITE_API_TOKEN" \
     -d '{"name":"my-site"}'

# 2. Push your content
cd ~/my-site-content
git init -b main
echo "<h1>Hello</h1>" > index.html
git add . && git commit -m "init"
git remote add minisite ~/minisite/repos/my-site.git
git push minisite main --force

# 3. Visit the site
curl http://localhost:9999/my-site/
```

### Management

```bash
# List sites
curl -H "Authorization: Bearer $MINISITE_API_TOKEN" \
     http://localhost:9999/api/repos

# Delete a site
curl -X DELETE http://localhost:9999/api/repos/my-site \
     -H "Authorization: Bearer $MINISITE_API_TOKEN"

# Force redeploy
curl -X POST http://localhost:9999/api/repos/my-site/deploy \
     -H "Authorization: Bearer $MINISITE_API_TOKEN"
```

### Other endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /` | Home page with site cards |
| `GET /howto` | Markdown docs for AI agents |
| `GET /health` | Health check |
| `GET /site-name/` | Directory listing or index.html |
| `GET /site-name/path/to/file` | Static file |

## Configuration

Edit `config.docker.toml`:

- `server.host` / `server.port` — listen address
- `server.github_url` — GitHub link in footer
- `git.repos_dir` / `git.worktrees_dir` — where data lives
- `static_files.auto_index` — enable directory listing
- `static_files.index_template` — template for directory pages

After editing, restart: `docker-compose restart`

## Authentication

The API (`/api/*`) requires a Bearer token. Sites themselves are public.

- Token goes in `contrib/.env` as `MINISITE_API_TOKEN`
- Or set `server.api_token` in config
- Without a token, API is open — only suitable for localhost

## Troubleshooting

### Port already in use

```bash
# Rootless podman sometimes holds the port
docker-compose up -d   # retry
```

### Can't connect from another machine

```bash
# Check firewall
sudo ufw allow 9999/tcp
```

### View logs

```bash
docker-compose logs -f
```

## License

MIT
