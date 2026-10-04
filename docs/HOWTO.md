# minisite — HowTo

> One-page reference for AI agents and scripts. Fetch it with `curl http://HOST:PORT/howto`.

## What this is

A lightweight self-hosted static-site host. You `git push` HTML/JS/CSS, and it serves them at `http://HOST:PORT/{repo-name}/` from a single port.

## Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET    | `/`                          | Service info (small JSON) |
| GET    | `/health`                    | Health check, returns `ok` |
| GET    | `/howto`                     | **This Markdown document** |
| GET    | `/api/repos`                 | List all repositories |
| POST   | `/api/repos`                 | Create a repository, body `{"name":"x"}` |
| DELETE | `/api/repos/{name}`          | Delete a repository |
| POST   | `/api/repos/{name}/deploy`   | Force-redeploy a repository |
| GET    | `/{repo}/`                   | Directory index (or auto-generated listing) |
| GET    | `/{repo}/path/to/file`       | Serve a file from the worktree |

## End-to-end: deploy your first site

```bash
# 1. Create the repo on the server
curl -X POST http://HOST:PORT/api/repos \
     -H 'Content-Type: application/json' \
     -d '{"name":"my-site"}'

# 2. Locally, prepare your site content
mkdir ~/my-site && cd ~/my-site
echo '<h1>Hello from minisite</h1>' > index.html
git init -b main
git add . && git commit -m 'init'

# 3. Add minisite as a git remote
#    The bare-repo path is whatever the server uses, e.g.:
#      /var/lib/minisite/repos/my-site.git
#      ssh://user@host/srv/minisite/repos/my-site.git   (if SSH is set up)
git remote add minisite /var/lib/minisite/repos/my-site.git

# 4. Push (--force is often needed the first time, because the server
#    creates an initial empty commit when the repo is registered)
git push minisite main --force

# 5. Verify
curl http://HOST:PORT/my-site/
open http://HOST:PORT/my-site/    # in a browser
```

## Update an existing site

```bash
cd ~/my-site
# edit index.html ...
git add . && git commit -m 'tweak'
git push minisite main            # no --force needed after the first push
```

The post-receive hook (auto-installed at repo creation) runs `git checkout -f main` against the worktree on every push, so updates appear immediately.

## Manage from the CLI (server-side)

```bash
minisite-cli list                  # list all repos
minisite-cli create my-site        # create
minisite-cli info my-site          # show paths, branch, timestamps
minisite-cli deploy my-site        # force re-checkout (e.g., after fixing permissions)
minisite-cli delete my-site        # remove bare repo + worktree
minisite-cli init --output config.toml   # generate starter config
```

## Repository name rules

- 1–100 characters
- Allowed: `a-z A-Z 0-9 _ -`
- Cannot start with `.` or `-`
- No slashes or dots inside

Invalid examples that return HTTP 400: `../etc`, `..hidden`, `-foo`, `my.site`.

## Gotchas

- **First push needs `--force`.** The server creates an empty initial commit when you register a repo; your local history diverges from that, so plain `git push` is rejected. Use `git push --force` once, then normal pushes work.
- **Only the default branch deploys.** Default is `main` (configurable via `git.default_branch` in `config.toml`). Pushes to other branches are accepted by the bare repo but ignored by the post-receive hook.
- **No build step.** Whatever is in your repo gets served verbatim. Run your bundler/minifier before committing.
- **Path traversal is blocked.** `/my-site/../blog/` returns 404.
- **Unknown repos return 404**, not 403, so the API cannot be used to enumerate names.
- **Directory listing is automatic** unless an `index.html` exists in that directory (in which case `index.html` is served).
- **`index.html` at the repo root makes `/{repo}/` serve it directly** instead of the listing.

## Curl recipes

```bash
# List repos
curl -s http://HOST:PORT/api/repos | jq

# Create
curl -s -X POST http://HOST:PORT/api/repos \
     -H 'Content-Type: application/json' \
     -d '{"name":"blog"}' | jq

# Delete
curl -s -X DELETE http://HOST:PORT/api/repos/blog | jq

# Force deploy
curl -s -X POST http://HOST:PORT/api/repos/blog/deploy | jq

# Fetch a file
curl -s http://HOST:PORT/blog/index.html

# Browse (directory listing)
curl -s http://HOST:PORT/blog/
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
howto_file = "/etc/minisite/HOWTO.md"
```

If the file is missing or unreadable, `/howto` still returns `200` with a short
message naming the offending path, so discovery never silently breaks.
