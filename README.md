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

# Start the service
docker-compose up -d --build
```

The service runs on port **9999**. Sites are stored in `~/gitlook/` on the host.

On first access, visit `http://localhost:9999/setup` to create the admin account.

## Usage

### Deploy a site

```bash
# 1. Create a repository via API (needs login)
# First login to get a session cookie, then use it:
curl -c cookies.txt -X POST http://localhost:9999/login \
     -d 'username=admin&password=yourpassword&next=/admin'

curl -b cookies.txt -X POST http://localhost:9999/api/repos \
     -H 'Content-Type: application/json' \
     -d '{"name":"my-site"}'

# 2. Push your content
cd ~/my-site-content
git init -b main
echo "<h1>Hello</h1>" > index.html
git add . && git commit -m "init"
git remote add gitlook ~/gitlook/repos/my-site.git
git push gitlook main --force

# 3. Visit the site
curl http://localhost:9999/my-site/
```

### Management

```bash
# List sites (login first to get cookies)
curl -b cookies.txt http://localhost:9999/api/repos

# Delete a site
curl -b cookies.txt -X DELETE http://localhost:9999/api/repos/my-site

# Force redeploy
curl -b cookies.txt -X POST http://localhost:9999/api/repos/my-site/deploy
```

### Other endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /` | Home page with site cards |
| `GET /howto` | Markdown docs for AI agents |
| `GET /health` | Health check |
| `GET /site-name/` | Directory listing or index.html |
| `GET /site-name/path/to/file` | Static file |
| `GET /setup` | First-time admin setup (redirects to /login if admin exists) |
| `GET /login` | Login page |
| `POST /logout` | Logout |
| `GET /admin` | Admin panel (manage users, change password) |

## Configuration

Edit `config.docker.toml`:

- `server.host` / `server.port` — listen address
- `server.github_url` — GitHub link in footer
- `git.repos_dir` / `git.worktrees_dir` — where data lives
- `git.db_path` — SQLite database for users/sessions
- `static_files.auto_index` — enable directory listing
- `static_files.index_template` — template for directory pages
- `static_files.home_template` — template for home page
- `static_files.setup_template` — template for first-time setup
- `static_files.login_template` — template for login page
- `static_files.admin_template` — template for admin panel

After editing, restart: `docker-compose restart`

## Authentication

The management API (`/api/*`) and admin panel (`/admin/*`) require a session cookie.

- First visit: go to `/setup` to create the admin account
- Subsequent visits: log in at `/login` — a `gitlook_session` cookie is set
- Use the cookie for API calls: `curl -b cookies.txt ...`
- Session expires after 30 days; change password to invalidate all sessions
- Without a valid session, `/api/*` returns `401`, `/admin/*` redirects to `/login`

To reset a forgotten password: log in as another admin at `/admin` and use the "修改密码" form, or delete `gitlook.db` and restart to re-run `/setup`.

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
