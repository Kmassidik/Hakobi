use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use super::{authenticated, AppState};
use hakobi::public_source::{fetch_example_environment, validate_discovery_input};

#[derive(Deserialize)]
pub(crate) struct PublicEnvironmentQuery {
    repo: String,
    branch: String,
    #[serde(default)]
    path: String,
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
