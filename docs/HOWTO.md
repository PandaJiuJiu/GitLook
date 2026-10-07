# Gitlook — HowTo

> One-page reference for AI agents and scripts. Fetch it with `curl http://HOST:PORT/howto`.

## What this is

A lightweight self-hosted static-site host. You `git push` HTML/JS/CSS, and it serves them at `http://HOST:PORT/{repo-name}/` from a single port.

## Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET    | `/`                          | Service info (small JSON) |
| GET    | `/health`                    | Health check, returns `ok` |
| GET    | `/howto`                     | **This Markdown document** |
| GET    | `/api/repos`                 | List repositories — **needs token** |
| POST   | `/api/repos`                 | Create a repo, body `{"name":"x"}` — **needs token** |
| DELETE | `/api/repos/{name}`          | Delete a repository — **needs token** |
| POST   | `/api/repos/{name}/deploy`   | Force-redeploy — **needs token** |
| GET    | `/{repo}/`                   | Directory index (or auto-generated listing) — public |
| GET    | `/{repo}/path/to/file`       | Serve a file from the worktree — public |

| Method | Path | Purpose |
|--------|------|---------|
| GET    | `/`                          | Service info (small JSON) |
| GET    | `/health`                    | Health check, returns `ok` |
| GET    | `/howto`                     | **This Markdown document** |
| GET    | `/api/repos`                 | List repositories — **needs login** |
| POST   | `/api/repos`                 | Create a repo, body `{"name":"x"}` — **needs login** |
| DELETE | `/api/repos/{name}`          | Delete a repository — **needs login** |
| POST   | `/api/repos/{name}/deploy`   | Force-redeploy — **needs login** |
| GET    | `/{repo}/`                   | Directory index (or auto-generated listing) — public |
| GET    | `/{repo}/path/to/file`       | Serve a file from the worktree — public |
| GET    | `/setup`                     | First-time admin setup — public (redirects to /login if admin exists) |
| GET    | `/login`                     | Login page — public |
| POST   | `/logout`                    | Logout — needs login |
| GET    | `/admin`                     | Admin panel — needs login |
| POST   | `/admin`                     | Create user — needs login |
| POST   | `/admin/password`            | Change own password — needs login |
| DELETE | `/admin/users/{id}`          | Delete user — needs login |

## Authentication

Hosted sites are public; the management API and admin panel are not. They require a session cookie.

**How it works:**
1. First visit → go to `/setup` to create the admin account
2. Login at `/login` with username + password → server sets `gitlook_session` cookie (HttpOnly, SameSite=Strict, 30-day expiry)
3. Include cookie in subsequent requests: `curl -b cookies.txt ...` or let browser handle it automatically

**Cookie format:**
```bash
# Login and save cookies
curl -c cookies.txt -X POST http://HOST:PORT/login \
     -d 'username=admin&password=yourpass&next=/admin'

# Use cookies for authenticated requests
curl -b cookies.txt http://HOST:PORT/api/repos
```

**Token comparison is constant-time.** Passwords are hashed with Argon2id (PHC format).

**Session management:**
- Session expires after 30 days of inactivity
- Changing password invalidates all sessions for that user
- Deleting a user cascades to delete their sessions

**If you forget your password:**
- Log in as another admin at `/admin` → use "修改密码" to reset the target user
- Or delete `gitlook.db` and restart the server → `/setup` will run again

**Without a valid session:** `/api/*` returns `401` JSON; `/admin/*` redirects to `/login?next=...`

## End-to-end: deploy your first site

```bash
# 1. Create the repo on the server (needs token)
curl -X POST http://HOST:PORT/api/repos \
     -H "Authorization: Bearer $MINISITE_API_TOKEN" \
     -H 'Content-Type: application/json' \
     -d '{"name":"my-site"}'

# 2. Locally, prepare your site content
mkdir ~/my-site && cd ~/my-site
echo '<h1>Hello from Gitlook</h1>' > index.html
git init -b main
git add . && git commit -m 'init'

# 3. Add Gitlook as a git remote
#    路径要用宿主机上的那个，不是容器里的 /var/lib/Gitlook。
#    仓库目录由宿主机 ~/Gitlook/repos 挂进容器，所以直接写宿主机路径：
#      ~/Gitlook/repos/my-site.git
git remote add Gitlook ~/Gitlook/repos/my-site.git

# 4. Push (--force is often needed the first time, because the server
#    creates an initial empty commit when the repo is registered)
git push Gitlook main --force

# 5. Verify
curl http://HOST:PORT/my-site/
open http://HOST:PORT/my-site/    # in a browser
```

## Update an existing site

```bash
cd ~/my-site
# edit index.html ...
git add . && git commit -m 'tweak'
git push Gitlook main            # no --force needed after the first push
```

The post-receive hook (auto-installed at repo creation) runs `git checkout -f main` against the worktree on every push, so updates appear immediately.

## Manage from the CLI (server-side)

```bash
Gitlook-cli list                  # list all repos
Gitlook-cli create my-site        # create
Gitlook-cli info my-site          # show paths, branch, timestamps
Gitlook-cli deploy my-site        # force re-checkout (e.g., after fixing permissions)
Gitlook-cli delete my-site        # remove bare repo + worktree
Gitlook-cli init --output config.toml   # generate starter config
```

## Repository name rules

- 1–100 characters
- Allowed: `a-z A-Z 0-9 _ -`
- Cannot start with `.` or `-`
- No slashes or dots inside

Invalid examples that return HTTP 400: `../etc`, `..hidden`, `-foo`, `my.site`.

## Gotchas

- **`/api/*` and `/admin/*` need a session cookie.** They return `401` (API) or redirect to `/login` (admin) without a valid `gitlook_session` cookie. See [Authentication](#authentication).
- **First push needs `--force`.** The server creates an empty initial commit when you register a repo; your local history diverges from that, so plain `git push` is rejected. Use `git push --force` once, then normal pushes work.
- **Only the default branch deploys.** Default is `main` (configurable via `git.default_branch` in `config.toml`). Pushes to other branches are accepted by the bare repo but ignored by the post-receive hook.
- **No build step.** Whatever is in your repo gets served verbatim. Run your bundler/minifier before committing.
- **Path traversal is blocked.** `/my-site/../blog/` returns 404.
- **Unknown repos return 404**, not 403, so the API cannot be used to enumerate names.
- **Directory listing is automatic** unless an `index.html` exists in that directory (in which case `index.html` is served).
- **`index.html` at the repo root makes `/{repo}/` serve it directly** instead of the listing.

## Curl recipes

```bash
# First: login and save cookies
curl -c cookies.txt -s -X POST http://HOST:PORT/login \
     -d 'username=admin&password=yourpass&next=/admin'

# List repos
curl -s -b cookies.txt http://HOST:PORT/api/repos | jq

# Create
curl -s -b cookies.txt -X POST http://HOST:PORT/api/repos \
     -H 'Content-Type: application/json' \
     -d '{"name":"blog"}' | jq

# Delete
curl -s -b cookies.txt -X DELETE http://HOST:PORT/api/repos/blog | jq

# Force deploy
curl -s -b cookies.txt -X POST http://HOST:PORT/api/repos/blog/deploy | jq

# Fetch a file (public — no cookie needed)
curl -s http://HOST:PORT/blog/index.html

# Browse (directory listing, public — no cookie needed)
curl -s http://HOST:PORT/blog/
```

Store the cookies once per shell to keep the commands short:

```bash
# Login once, then reuse cookies.txt
curl -c cookies.txt -X POST http://HOST:PORT/login \
     -d 'username=admin&password=yourpass&next=/admin'
```

## What happens on `git push` (internals)

1. Client pushes to `repos/<name>.git/` (bare repo)
2. Bare repo's `hooks/post-receive` script fires with `(oldrev, newrev, refname)` on stdin
3. If `refname == refs/heads/<default_branch>`, hook runs:
   ```bash
   git --git-dir=$GIT_DIR --work-tree=$WORKTREE checkout -f main
   ```
   where `GIT_DIR` is computed from the hook's own location and `WORKTREE` was templated in at repo-creation time.
4. Worktree contents are replaced with the new tree
5. The HTTP server reads from the worktree on subsequent requests — no restart, no cache flush

## Filesystem layout

```
$WORKTREES_DIR/
└── <repo-name>/
    ├── index.html
    ├── style.css
    └── ...

$REPOS_DIR/
└── <repo-name>.git/
    ├── HEAD
    ├── objects/...
    ├── refs/heads/main
    └── hooks/
        └── post-receive     # auto-generated by create_repo
```

## Customizing this document

`GET /howto` serves a plain Markdown file from disk — the path comes from
`server.howto_file` in `config.toml` (default `docs/HOWTO.md`). Edit it in
place; changes are picked up on the next request with no restart or rebuild.

```toml
[server]
howto_file = "/etc/Gitlook/HOWTO.md"
```

If the file is missing or unreadable, `/howto` still returns `200` with a short
message naming the offending path, so discovery never silently breaks.
