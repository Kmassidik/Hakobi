use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hakobi::agent_build;

use super::{authorized, docker, error_response, install, unauthorized, AgentState};

/// Builds the agent router.
///
/// Every route except `/healthz` requires the bearer token. The Docker routes
/// are read and operate endpoints for the fixed host socket; none of them
/// proxies raw Docker API calls or accepts a user-supplied endpoint.
pub(super) fn app(state: AgentState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/readiness", get(readiness))
        .route("/v1/docker/metrics", get(docker::docker_metrics))
        .route("/v1/system/metrics", get(system_metrics))
        .route("/v1/docker/version", get(docker::docker_version))
        .route("/v1/docker/containers", get(docker::docker_containers))
        .route(
            "/v1/docker/containers/{name}/inspect",
            get(docker::docker_container_inspect),
        )
        .route(
            "/v1/docker/containers/{name}/logs",
            get(docker::docker_container_logs),
        )
        .route(
            "/v1/docker/containers/{name}/stats",
            get(docker::docker_container_stats),
        )
        .route("/v1/docker/snapshot", get(docker::docker_snapshot))
        .route("/v1/docker/containers/run", post(docker::docker_run))
        .route("/build/dockerfile", post(agent_build::build_dockerfile))
        .route("/build/compose", post(agent_build::build_compose))
        .route("/build/nixpacks", post(agent_build::build_nixpacks))
        .route(
            "/v1/docker/containers/{name}/start",
            post(docker::docker_start),
        )
        .route(
            "/v1/docker/containers/{name}/stop",
            post(docker::docker_stop),
        )
        .route(
            "/v1/docker/containers/{name}/restart",
            post(docker::docker_restart),
        )
        .route(
            "/v1/docker/containers/{name}/delete",
            post(docker::docker_delete),
        )
        .route("/v1/install/status", get(install::install_status))
        .route("/v1/install/{tool}", post(install::install))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    StatusCode::OK
}

async fn readiness(State(state): State<AgentState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    match tokio::task::spawn_blocking(hakobi::install::detect).await {
        Ok(readiness) => Json(readiness).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn system_metrics(State(state): State<AgentState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let metrics = state.system.lock().await.snapshot();
    Json(metrics).into_response()
}
