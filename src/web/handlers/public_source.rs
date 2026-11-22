use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use super::{authenticated, AppState};
use hakobi::{
    host_agent::PublicSourceScanRequest,
    public_source::{fetch_example_environment, validate_discovery_input},
};

#[derive(Deserialize)]
pub(crate) struct PublicEnvironmentQuery {
    repo: String,
    branch: String,
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
pub(crate) struct PublicScanBody {
    host_id: String,
    git_url: String,
    branch: Option<String>,
    csrf_token: String,
}

pub(crate) async fn scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PublicScanBody>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !super::post_is_allowed(&state, &headers, &body.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Err(error) = hakobi::buildrunner::validate_public_git_url(&body.git_url) {
        return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
    }
    if let Some(branch) = body.branch.as_deref() {
        if let Err(error) = hakobi::buildrunner::validate_branch(branch) {
            return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    }
    let host_id = body.host_id.clone();
    let enabled = super::with_catalog(&state, move |catalog| {
        Ok(catalog
            .list_host_agents()?
            .into_iter()
            .any(|host| host.id == host_id && host.enabled))
    })
    .await;
    if !matches!(enabled, Ok(true)) {
        return (StatusCode::BAD_REQUEST, "Select an enabled host.").into_response();
    }
    let endpoint = format!("agent://{}", body.host_id);
    let request = PublicSourceScanRequest {
        git_url: body.git_url,
        branch: body.branch,
    };
    match state.backend.scan_public_source(&endpoint, &request).await {
        Ok(scan) => Json(scan).into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    }
}

pub(crate) async fn environment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PublicEnvironmentQuery>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Err(error) = validate_discovery_input(&query.repo, &query.branch, &query.path) {
        return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
    }
    match fetch_example_environment(&query.repo, &query.branch, &query.path).await {
        Ok(environment) => Json(serde_json::json!({
            "found": environment.is_some(),
            "env": environment.unwrap_or_default(),
        }))
        .into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, error.to_string()).into_response(),
    }
}
