use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::server::AppState;

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRepoRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct RepoListResponse {
    pub repos: Vec<RepoSummary>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct RepoSummary {
    pub name: String,
    pub default_branch: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list_repos(State(state): State<AppState>) -> Response {
    let git = &state.git;
    match git.list_repos().await {
        Ok(repos) => {
            let summaries: Vec<RepoSummary> = repos.into_iter().map(|r| RepoSummary {
                name: r.name,
                default_branch: r.default_branch,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }).collect();
            let response = ApiResponse {
                success: true,
                data: Some(RepoListResponse {
                    count: summaries.len(),
                    repos: summaries,
                }),
                error: None,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to list repos: {}", e);
            let response = ApiResponse::<()> {
                success: false,
                data: None,
                error: Some(e.to_string()),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response)).into_response()
        }
    }
}

pub async fn create_repo(
    State(state): State<AppState>,
    Json(req): Json<CreateRepoRequest>,
) -> Response {
    let git = &state.git;
    info!("Creating repository: {}", req.name);
    match git.create_repo(&req.name).await {
        Ok(repo) => {
            info!("Repository created: {}", req.name);
            let response = ApiResponse {
                success: true,
                data: Some(RepoSummary {
                    name: repo.name,
                    default_branch: repo.default_branch,
                    created_at: repo.created_at,
                    updated_at: repo.updated_at,
                }),
                error: None,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to create repo {}: {}", req.name, e);
            let status = if e.to_string().contains("AlreadyExists") {
                StatusCode::CONFLICT
            } else if e.to_string().contains("InvalidName") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let response = ApiResponse::<()> {
                success: false,
                data: None,
                error: Some(e.to_string()),
            };
            (status, Json(response)).into_response()
        }
    }
}

pub async fn delete_repo(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let git = &state.git;
    info!("Deleting repository: {}", name);
    match git.delete_repo(&name).await {
        Ok(_) => {
            info!("Repository deleted: {}", name);
            let response = ApiResponse {
                success: true,
                data: Some(()),
                error: None,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to delete repo {}: {}", name, e);
            let status = if e.to_string().contains("NotFound") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let response = ApiResponse::<()> {
                success: false,
                data: None,
                error: Some(e.to_string()),
            };
            (status, Json(response)).into_response()
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DeployResponse {
    pub repo_name: String,
    pub success: bool,
    pub message: String,
    pub commit_hash: String,
}

pub async fn trigger_deploy(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let git = &state.git;
    info!("Manual deploy triggered for: {}", name);
    match git.deploy(&name).await {
        Ok(result) => {
            let response = ApiResponse {
                success: true,
                data: Some(DeployResponse {
                    repo_name: result.repo_name,
                    success: result.success,
                    message: result.message,
                    commit_hash: result.commit_hash,
                }),
                error: None,
            };
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            warn!("Failed to deploy {}: {}", name, e);
            let status = if e.to_string().contains("NotFound") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let response = ApiResponse::<()> {
                success: false,
                data: None,
                error: Some(e.to_string()),
            };
            (status, Json(response)).into_response()
        }
    }
}