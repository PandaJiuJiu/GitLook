//! 认证相关 HTTP handlers。

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;
use tracing::{info, warn};

use crate::auth::session::AuthUser;
use crate::auth::{db::Database, password::{hash, verify}, session::{generate_token, set_session_cookie, clear_session_cookie}};
use crate::server::{AppState, static_files::StaticFileServer};

/// 通用表单错误响应（HTML）。
fn form_error(html: &str, msg: &str) -> Response {
    let replaced = html.replace("<!-- FORM_ERROR -->", &format!("<p class=\"error\">{}</p>", msg));
    (StatusCode::BAD_REQUEST, Html(replaced)).into_response()
}

/// 通用模板渲染辅助。
fn render_template(server: &StaticFileServer, name: &str, ctx: TeraContext) -> Response {
    match server.render_template(name, &ctx) {
        Ok(html) => (StatusCode::OK, Html(html)).into_response(),
        Err(e) => {
            tracing::error!("Failed to render {} template: {:?}", name, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                format!("Template render error: {}", e),
            )
                .into_response()
        }
    }
}

/// --- /setup GET: 首次运行创建管理员 ---

pub async fn setup_get(State(state): State<AppState>) -> Response {
    // 检查是否已有用户
    let has_users = state.db.count_users().await.unwrap_or(0) > 0;
    if has_users {
        // 已有用户，重定向到登录页
        return Redirect::to("/login").into_response();
    }

    let ctx = TeraContext::new();
    render_template(&state.static_files, "setup", ctx)
}

/// /setup POST: 创建第一个管理员账号
#[derive(Deserialize)]
pub struct SetupForm {
    username: String,
    password: String,
    confirm: String,
}

pub async fn setup_post(State(state): State<AppState>, Form(form): Form<SetupForm>) -> Response {
    // 检查是否已有用户
    if state.db.count_users().await.unwrap_or(0) > 0 {
        return Redirect::to("/login").into_response();
    }

    // 验证
    if form.username.trim().is_empty() {
        return form_error(SETUP_TEMPLATE, "用户名不能为空");
    }
    if form.password.is_empty() {
        return form_error(SETUP_TEMPLATE, "密码不能为空");
    }
    if form.password != form.confirm {
        return form_error(SETUP_TEMPLATE, "两次输入的密码不一致");
    }
    if form.password.len() < 8 {
        return form_error(SETUP_TEMPLATE, "密码至少 8 位");
    }

    // 创建用户
    let password_hash = hash(&form.password);
    match state.db.create_user(&form.username, &password_hash).await {
        Ok(user_id) => {
            info!("Created initial admin user: {} (id={})", form.username, user_id);
            // 创建 session 并设 cookie
            let token = generate_token();
            if let Err(e) = state.db.create_session(&token, user_id).await {
                warn!("Failed to create session after setup: {}", e);
            }
            let mut resp = Redirect::to("/admin").into_response();
            set_session_cookie(&mut resp, &token);
            resp
        }
        Err(e) => {
            warn!("Failed to create user: {}", e);
            form_error(SETUP_TEMPLATE, "创建用户失败（用户名可能已存在）")
        }
    }
}

/// --- /login GET/POST ---

#[derive(Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

pub async fn login_get(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Response {
    // 检查是否已有用户
    if state.db.count_users().await.unwrap_or(0) == 0 {
        return Redirect::to("/setup").into_response();
    }

    let mut ctx = TeraContext::new();
    if let Some(next) = query.next {
        ctx.insert("next", &next);
    }
    render_template(&state.static_files, "login", ctx)
}

pub async fn login_post(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
    Form(form): Form<LoginForm>,
) -> Response {
    // 检查是否已有用户
    if state.db.count_users().await.unwrap_or(0) == 0 {
        return Redirect::to("/setup").into_response();
    }

    // 验证
    if form.username.trim().is_empty() || form.password.is_empty() {
        let mut ctx = TeraContext::new();
        if let Some(next) = query.next {
            ctx.insert("next", &next);
        }
        return render_template(&state.static_files, "login", ctx); // 模板里会显示通用错误
    }

    // 查用户
    let (user_id, password_hash) = match state.db.find_user_by_name(&form.username).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            // 用户不存在：为了不泄露用户是否存在，用同一个错误提示
            let mut ctx = TeraContext::new();
            if let Some(next) = query.next {
                ctx.insert("next", &next);
            }
            ctx.insert("error", "用户名或密码错误");
            return render_template(&state.static_files, "login", ctx);
        }
        Err(e) => {
            warn!("DB error finding user: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // 验证密码
    if !verify(&form.password, &password_hash) {
        let mut ctx = TeraContext::new();
        if let Some(next) = query.next {
            ctx.insert("next", &next);
        }
        ctx.insert("error", "用户名或密码错误");
        return render_template(&state.static_files, "login", ctx);
    }

    // 更新最后登录时间
    if let Err(e) = state.db.update_last_login(user_id).await {
        warn!("Failed to update last_login: {}", e);
    }

    // 创建 session
    let token = generate_token();
    if let Err(e) = state.db.create_session(&token, user_id).await {
        warn!("Failed to create session: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
    }

    info!("User logged in: {} (id={})", form.username, user_id);

    let next = query.next.unwrap_or_else(|| "/admin".to_string());
    let mut resp = Redirect::to(&next).into_response();
    set_session_cookie(&mut resp, &token);
    resp
}

/// --- /logout POST ---

pub async fn logout_post(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // 从 cookie 取 token 并删 DB
    if let Some(token) = crate::auth::session::extract_session_token(&headers) {
        if let Err(e) = state.db.delete_session(&token).await {
            warn!("Failed to delete session: {}", e);
        }
    }

    let mut resp = Redirect::to("/").into_response();
    clear_session_cookie(&mut resp);
    resp
}

/// --- /admin GET: 用户管理页面 ---

pub async fn admin_get(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
) -> Response {
    let users = match state.db.list_users().await {
        Ok(u) => u,
        Err(e) => {
            warn!("Failed to list users: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    let mut ctx = TeraContext::new();
    ctx.insert("current_user_id", &user.user_id);
    ctx.insert("current_username", &user.username);

    // 序列化用户列表
    #[derive(Serialize)]
    struct UserRow {
        id: i64,
        username: String,
        created_at: String,
        last_login_at: Option<String>,
    }

    let users_display: Vec<UserRow> = users
        .into_iter()
        .map(|(id, username, created_at, last_login_at)| UserRow {
            id,
            username,
            created_at: chrono::DateTime::<chrono::Utc>::from_timestamp(created_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "—".to_string()),
            last_login_at: last_login_at
                .and_then(|ts| chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0))
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string()),
        })
        .collect();

    ctx.insert("users", &users_display);
    render_template(&state.static_files, "admin", ctx)
}

/// --- /admin POST: 创建新用户 ---

#[derive(Deserialize)]
pub struct CreateUserForm {
    username: String,
    password: String,
    confirm: String,
}

pub async fn admin_user_post(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Form(form): Form<CreateUserForm>,
) -> Response {
    if form.username.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "用户名不能为空").into_response();
    }
    if form.password.is_empty() {
        return (StatusCode::BAD_REQUEST, "密码不能为空").into_response();
    }
    if form.password != form.confirm {
        return (StatusCode::BAD_REQUEST, "两次输入的密码不一致").into_response();
    }
    if form.password.len() < 8 {
        return (StatusCode::BAD_REQUEST, "密码至少 8 位").into_response();
    }

    let password_hash = hash(&form.password);
    match state.db.create_user(&form.username, &password_hash).await {
        Ok(_) => {
            info!("Admin {} created user: {}", user.username, form.username);
            Redirect::to("/admin").into_response()
        }
        Err(e) => {
            warn!("Failed to create user: {}", e);
            (StatusCode::CONFLICT, "用户名已存在").into_response()
        }
    }
}

/// --- /admin/password POST: 修改自己密码 ---

#[derive(Deserialize)]
pub struct ChangePasswordForm {
    current: String,
    new: String,
    confirm: String,
}

pub async fn admin_password_post(
    AuthUser(user): AuthUser,
    State(state): State<AppState>,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    // 取当前用户的 hash
    let (_, current_hash) = match state.db.find_user_by_name(&user.username).await {
        Ok(Some(u)) => u,
        Ok(None) => return (StatusCode::NOT_FOUND, "用户不存在").into_response(),
        Err(e) => {
            warn!("DB error: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
        }
    };

    // 验证当前密码
    if !verify(&form.current, &current_hash) {
        return (StatusCode::BAD_REQUEST, "当前密码错误").into_response();
    }

    if form.new.is_empty() {
        return (StatusCode::BAD_REQUEST, "新密码不能为空").into_response();
    }
    if form.new != form.confirm {
        return (StatusCode::BAD_REQUEST, "两次输入的新密码不一致").into_response();
    }
    if form.new.len() < 8 {
        return (StatusCode::BAD_REQUEST, "密码至少 8 位").into_response();
    }

    // 更新密码（会级联删该用户所有 session）
    let new_hash = hash(&form.new);
    if let Err(e) = state.db.update_password(user.user_id, &new_hash).await {
        warn!("Failed to update password: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
    }

    info!("User {} changed password", user.username);

    // 当前 session 已失效，重定向到登录页
    let mut resp = Redirect::to("/login").into_response();
    clear_session_cookie(&mut resp);
    resp
}

/// --- /admin/users/{id} DELETE: 删除用户 ---

pub async fn admin_user_delete(
    AuthUser(current_user): AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    // 不能删自己（会在前端也挡住，这里再守一层）
    if id == current_user.user_id {
        return (StatusCode::BAD_REQUEST, "不能删除自己").into_response();
    }

    if let Err(e) = state.db.delete_user(id).await {
        warn!("Failed to delete user {}: {}", id, e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response();
    }

    info!("Admin {} deleted user id={}", current_user.username, id);
    Redirect::to("/admin").into_response()
}

// ===== 内置模板（fallback，磁盘模板不存在时用）=====

const SETUP_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Gitlook — 首次设置</title>
<style>
:root { --bg:#f7f8fa; --card:#fff; --text:#1a1d21; --muted:#6b7280; --border:#e5e7eb; --accent:#2563eb; --accent-soft:#eff4ff; --shadow:0 1px 2px rgba(0,0,0,.04),0 4px 12px rgba(0,0,0,.04); }
@media(prefers-color-scheme:dark){:root{--bg:#0e1116;--card:#171a21;--text:#e6e8ec;--muted:#9aa3b2;--border:#262b34;--accent:#6ea8fe;--accent-soft:#16233b;--shadow:0 1px 2px rgba(0,0,0,.3),0 4px 12px rgba(0,0,0,.25);}}
*{box-sizing:border-box}body{margin:0;padding:3rem 1.5rem 4rem;background:var(--bg);color:var(--text);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,"Noto Sans",sans-serif;line-height:1.6}
.wrap{max-width:420px;margin:0 auto}.card{background:var(--card);border:1px solid var(--border);border-radius:12px;padding:2rem;box-shadow:var(--shadow)}
.logo{display:inline-flex;align-items:center;gap:.55rem;font-size:1.5rem;font-weight:650}.logo .dot{width:.6rem;height:.6rem;border-radius:50%;background:var(--accent)}
h1{margin:1.5rem 0 0;font-size:1.25rem}.desc{color:var(--muted);margin:.5rem 0 1.5rem}
label{display:block;margin-bottom:.35rem;font-size:.9rem}.input{width:100%;padding:.6rem .8rem;border:1px solid var(--border);border-radius:8px;background:var(--bg);color:var(--text);font-size:1rem}
.input:focus{outline:2px solid var(--accent);outline-offset:2px;border-color:var(--accent)}
.btn{width:100%;margin-top:1rem;padding:.7rem 1rem;background:var(--accent);color:#fff;border:none;border-radius:8px;font-size:1rem;font-weight:600;cursor:pointer}
.btn:hover{filter:brightness(1.05)}.error{color:#dc2626;font-size:.85rem;margin-top:.5rem}
</style>
</head>
<body>
<div class="wrap">
<div class="card">
<div class="logo"><span class="dot"></span> Gitlook</div>
<h1>首次设置</h1>
<p class="desc">检测到暂无管理员账号，请创建第一个管理员。</p>
<form method="post">
<label>用户名</label>
<input class="input" name="username" autocomplete="username" required>
<label>密码</label>
<input type="password" class="input" name="password" autocomplete="new-password" required minlength="8">
<label>确认密码</label>
<input type="password" class="input" name="confirm" autocomplete="new-password" required minlength="8">
<!-- FORM_ERROR -->
<button class="btn" type="submit">创建并登录</button>
</form>
</div>
</div>
</body>
</html>"#;

const LOGIN_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Gitlook — 登录</title>
<style>
:root { --bg:#f7f8fa; --card:#fff; --text:#1a1d21; --muted:#6b7280; --border:#e5e7eb; --accent:#2563eb; --accent-soft:#eff4ff; --shadow:0 1px 2px rgba(0,0,0,.04),0 4px 12px rgba(0,0,0,.04); }
@media(prefers-color-scheme:dark){:root{--bg:#0e1116;--card:#171a21;--text:#e6e8ec;--muted:#9aa3b2;--border:#262b34;--accent:#6ea8fe;--accent-soft:#16233b;--shadow:0 1px 2px rgba(0,0,0,.3),0 4px 12px rgba(0,0,0,.25);}}
*{box-sizing:border-box}body{margin:0;padding:3rem 1.5rem 4rem;background:var(--bg);color:var(--text);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,"Noto Sans",sans-serif;line-height:1.6}
.wrap{max-width:420px;margin:0 auto}.card{background:var(--card);border:1px solid var(--border);border-radius:12px;padding:2rem;box-shadow:var(--shadow)}
.logo{display:inline-flex;align-items:center;gap:.55rem;font-size:1.5rem;font-weight:650}.logo .dot{width:.6rem;height:.6rem;border-radius:50%;background:var(--accent)}
h1{margin:1.5rem 0 0;font-size:1.25rem}.desc{color:var(--muted);margin:.5rem 0 1.5rem}
label{display:block;margin-bottom:.35rem;font-size:.9rem}.input{width:100%;padding:.6rem .8rem;border:1px solid var(--border);border-radius:8px;background:var(--bg);color:var(--text);font-size:1rem}
.input:focus{outline:2px solid var(--accent);outline-offset:2px;border-color:var(--accent)}
.btn{width:100%;margin-top:1rem;padding:.7rem 1rem;background:var(--accent);color:#fff;border:none;border-radius:8px;font-size:1rem;font-weight:600;cursor:pointer}
.btn:hover{filter:brightness(1.05)}.error{color:#dc2626;font-size:.85rem;margin-top:.5rem}
</style>
</head>
<body>
<div class="wrap">
<div class="card">
<div class="logo"><span class="dot"></span> Gitlook</div>
<h1>登录</h1>
<p class="desc">输入用户名和密码访问管理面板。</p>
<form method="post">
<input type="hidden" name="next" value="{{ next }}">
<label>用户名</label>
<input class="input" name="username" autocomplete="username" required autofocus>
<label>密码</label>
<input type="password" class="input" name="password" autocomplete="current-password" required>
{% if error %}<p class="error">{{ error }}</p>{% endif %}
<button class="btn" type="submit">登录</button>
</form>
</div>
</div>
</body>
</html>"#;

const ADMIN_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Gitlook — 管理面板</title>
<style>
:root { --bg:#f7f8fa; --card:#fff; --text:#1a1d21; --muted:#6b7280; --border:#e5e7eb; --accent:#2563eb; --accent-soft:#eff4ff; --shadow:0 1px 2px rgba(0,0,0,.04),0 4px 12px rgba(0,0,0,.04); }
@media(prefers-color-scheme:dark){:root{--bg:#0e1116;--card:#171a21;--text:#e6e8ec;--muted:#9aa3b2;--border:#262b34;--accent:#6ea8fe;--accent-soft:#16233b;--shadow:0 1px 2px rgba(0,0,0,.3),0 4px 12px rgba(0,0,0,.25);}}
*{box-sizing:border-box}body{margin:0;padding:3rem 1.5rem 4rem;background:var(--bg);color:var(--text);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,"Helvetica Neue",Arial,"Noto Sans",sans-serif;line-height:1.6}
.wrap{max-width:900px;margin:0 auto}
header{margin-bottom:2rem}.logo{display:inline-flex;align-items:center;gap:.55rem;font-size:1.5rem;font-weight:650}.logo .dot{width:.6rem;height:.6rem;border-radius:50%;background:var(--accent)}
.card{background:var(--card);border:1px solid var(--border);border-radius:12px;padding:1.5rem;box-shadow:var(--shadow);margin-bottom:1.5rem}
h2{margin:0 0 1rem;font-size:1.1rem}
.table-wrap{overflow-x:auto}table{width:100%;border-collapse:collapse}th,td{padding:.75rem 1rem;text-align:left;border-bottom:1px solid var(--border)}th{background:var(--accent-soft);color:var(--accent);font-weight:600;font-size:.85rem}tr:hover td{background:var(--accent-soft)}
.btn{display:inline-block;padding:.5rem 1rem;background:var(--accent);color:#fff;border:none;border-radius:6px;font-size:.9rem;font-weight:600;cursor:pointer;text-decoration:none}.btn:hover{filter:brightness(1.05)}
.btn-danger{background:#dc2626}.btn-danger:hover{filter:brightness(1.05)}
.btn-ghost{background:transparent;color:var(--accent);border:1px solid var(--accent)}.btn-ghost:hover{background:var(--accent-soft)}
.form-row{display:grid;grid-template-columns:1fr 1fr;gap:1rem;margin-bottom:1rem}@media(max-width:600px){.form-row{grid-template-columns:1fr}}
.input{width:100%;padding:.6rem .8rem;border:1px solid var(--border);border-radius:8px;background:var(--bg);color:var(--text);font-size:.95rem}
.input:focus{outline:2px solid var(--accent);outline-offset:2px;border-color:var(--accent)}
.actions{display:flex;gap:.5rem}.muted{color:var(--muted);font-size:.85rem}
footer{margin-top:2rem;color:var(--muted);font-size:.85rem;text-align:center}
</style>
</head>
<body>
<div class="wrap">
<header>
<div class="logo"><span class="dot"></span> Gitlook</div>
<p class="muted">管理面板 — 当前用户：<strong>{{ current_username }}</strong></p>
</header>

<div class="card">
<h2>用户列表</h2>
<div class="table-wrap">
<table>
<thead><tr><th style="width:60px">ID</th><th>用户名</th><th>创建时间</th><th>最后登录</th><th style="width:120px">操作</th></tr></thead>
<tbody>
{% for u in users %}
<tr>
<td>{{ u.id }}</td>
<td>{{ u.username }}{% if u.id == current_user_id %} <span class="muted">(当前)</span>{% endif %}</td>
<td>{{ u.created_at }}</td>
<td>{{ u.last_login_at | default(value="—") }}</td>
<td class="actions">
{% if u.id != current_user_id %}
<form method="post" action="/admin/users/{{ u.id }}" style="display:inline" onsubmit="return confirm('确定删除 {{ u.username }} 吗？');">
<button class="btn btn-danger" type="submit">删除</button>
</form>
{% else %}
<span class="muted">—</span>
{% endif %}
</td>
</tr>
{% else %}
<tr><td colspan="5" class="muted" style="text-align:center;padding:2rem">暂无用户</td></tr>
{% endfor %}
</tbody>
</table>
</div>
</div>

<div class="card">
<h2>新建用户</h2>
<form method="post" action="/admin">
<div class="form-row">
<div><label>用户名</label><input class="input" name="username" required></div>
<div><label>密码</label><input type="password" class="input" name="password" required minlength="8"></div>
</div>
<div class="form-row">
<div><label>确认密码</label><input type="password" class="input" name="confirm" required minlength="8"></div>
<div></div>
</div>
<button class="btn" type="submit">创建</button>
</form>
</div>

<div class="card">
<h2>修改密码</h2>
<form method="post" action="/admin/password">
<div class="form-row">
<div><label>当前密码</label><input type="password" class="input" name="current" required></div>
<div><label>新密码</label><input type="password" class="input" name="new" required minlength="8"></div>
</div>
<div class="form-row">
<div><label>确认新密码</label><input type="password" class="input" name="confirm" required minlength="8"></div>
<div></div>
</div>
<button class="btn" type="submit">更新密码</button>
</form>
</div>

<footer>
<a href="/">返回首页</a> · <form method="post" action="/logout" style="display:inline"><button class="btn-ghost" type="submit">登出</button></form>
</footer>
</div>
</body>
</html>"#;