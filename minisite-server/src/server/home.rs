//! 首页（`GET /`）：以卡片形式列出所有已托管的站点。
//!
//! 站点名、相对时间等展示用的字符串在 Rust 侧算好再传给模板——
//! Tera 处理 null 不可靠，直接把 Option 丢进去曾经导致模板渲染失败。

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use tera::Context as TeraContext;

use crate::server::AppState;

/// 卡片上要显示的字段，全部是已格式化好的字符串
#[derive(Clone, Serialize)]
pub struct SiteCard {
    pub name: String,
    pub href: String,
    pub branch: String,
    pub updated_display: String,
}

/// 机器可读的服务信息。浏览器看到 HTML，脚本想要 JSON 时带
/// `Accept: application/json` 即可（curl 默认 */*，会拿到 HTML）。
#[derive(Serialize)]
struct ServiceInfo {
    service: &'static str,
    version: &'static str,
    howto: &'static str,
    sites: Vec<SiteCard>,
    endpoints: Endpoints,
}

#[derive(Serialize)]
struct Endpoints {
    health: &'static str,
    api: ApiEndpoints,
}

#[derive(Serialize)]
struct ApiEndpoints {
    list_repos: &'static str,
    create_repo: &'static str,
    delete_repo: &'static str,
    deploy_repo: &'static str,
    auth: &'static str,
}

pub async fn home(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let repos = match state.git.list_repos().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to list repos for home page: {:?}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list repositories").into_response();
        }
    };

    // 只列出 worktree 还在的仓库。建仓时就会建好 worktree 并写入 README，
    // 所以这里挡的其实是"worktree 被手动删掉"的情况——那种卡片点进去会 404，
    // 出现在首页上只是让人以为站点坏了。
    let mut sites: Vec<SiteCard> = Vec::with_capacity(repos.len());
    for repo in repos {
        if !repo.worktree_path.is_dir() {
            continue;
        }
        sites.push(SiteCard {
            href: format!("/{}/", repo.name),
            name: repo.name,
            branch: repo.default_branch,
            updated_display: humanize(repo.updated_at),
        });
    }
    sites.sort_by(|a, b| a.name.cmp(&b.name));

    if wants_json(&headers) {
        return Json(ServiceInfo {
            service: "minisite",
            version: env!("CARGO_PKG_VERSION"),
            howto: "/howto",
            sites: sites.clone(),
            endpoints: Endpoints {
                health: "/health",
                api: ApiEndpoints {
                    list_repos: "GET /api/repos",
                    create_repo: "POST /api/repos",
                    delete_repo: "DELETE /api/repos/{name}",
                    deploy_repo: "POST /api/repos/{name}/deploy",
                    auth: "Authorization: Bearer <token>",
                },
            },
        })
            .into_response();
    }

    let ctx = TeraContext::from_serialize(serde_json::json!({
        "sites": sites,
        "site_count": sites.len(),
        "base_url": "/",
        "howto_url": "/howto",
        "api_repos_url": "/api/repos",
    })).unwrap_or_else(|e| {
        tracing::error!("Failed to build home template context: {:?}", e);
        TeraContext::new()
    });

    match state.static_files.render_home(&ctx) {
        Ok(html) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to render home template: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                format!("Home template render error: {}", e),
            )
                .into_response()
        }
    }
}

/// 只有显式要 JSON 时才给 JSON。curl 默认 `Accept: */*`，浏览器发 `text/html`，
/// 两者都拿不到 application/json，所以脚本需要显式声明。
fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false)
}

/// 把时间点变成"3 分钟前"这种人话
fn humanize(ts: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let Some(ts) = ts else {
        return "never updated".to_string();
    };

    let delta = chrono::Utc::now().signed_duration_since(ts);
    let secs = delta.num_seconds().max(0);

    let (n, unit) = if secs < 60 {
        return "just now".to_string();
    } else if secs < 3600 {
        (secs / 60, "minute")
    } else if secs < 86400 {
        (secs / 3600, "hour")
    } else if secs < 30 * 86400 {
        (secs / 86400, "day")
    } else {
        return ts.format("%Y-%m-%d").to_string();
    };

    let n = n.max(1);
    if n == 1 {
        format!("1 {} ago", unit)
    } else {
        format!("{} {}s ago", n, unit)
    }
}

#[cfg(test)]
mod tests {
    use super::humanize;

    #[test]
    fn none_is_never_updated() {
        assert_eq!(humanize(None), "never updated");
    }

    #[test]
    fn just_now() {
        let ts = chrono::Utc::now();
        assert_eq!(humanize(Some(ts)), "just now");
    }

    #[test]
    fn singular_and_plural() {
        let now = chrono::Utc::now();
        let one_hour = now - chrono::Duration::hours(1);
        let three_hours = now - chrono::Duration::hours(3);
        assert_eq!(humanize(Some(one_hour)), "1 hour ago");
        assert_eq!(humanize(Some(three_hours)), "3 hours ago");
    }

    #[test]
    fn old_dates_fall_back_to_absolute() {
        let ts = chrono::Utc::now() - chrono::Duration::days(120);
        // 超过 30 天就该给绝对日期，相对时间没有意义了
        assert_eq!(humanize(Some(ts)), ts.format("%Y-%m-%d").to_string());
    }

    #[test]
    fn clock_skew_does_not_produce_negative_ages() {
        // 服务器时间被往前调过时 signed_duration 为负，不能显示 "5 minutes ago"
        let future = chrono::Utc::now() + chrono::Duration::minutes(5);
        assert_eq!(humanize(Some(future)), "just now");
    }
}
