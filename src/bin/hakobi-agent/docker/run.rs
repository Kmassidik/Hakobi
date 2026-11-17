//! Authenticated Docker container creation with resource limits.

use std::time::Duration;

use anyhow::{bail, Result};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::{
    host_agent::{self, RunContainerRequest, RunContainerResponse},
    providers::{
        validate_container_name, validate_image_reference, ContainerBackend, DockerBackend,
        ResourceLimits,
    },
};

use super::super::{authorized, error_response, unauthorized, AgentState};
use super::docker_call;

const DOCKER_RUN_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) async fn docker_run(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<RunContainerRequest>, JsonRejection>,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            return error_response(StatusCode::BAD_REQUEST, rejection.body_text());
        }
    };
    if let Err(error) = validate_run_request(&request) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let backend = DockerBackend;
    match docker_call(
        backend.run_container(
            host_agent::DOCKER_SOCKET_ENDPOINT,
            &request.image,
            &request.name,
            request.container_port,
            request.host_port,
            &request.env,
            ResourceLimits {
                cpu: request.cpu_limit,
                memory_bytes: request.memory_limit_bytes,
            },
        ),
        DOCKER_RUN_TIMEOUT,
    )
    .await
    {
        Ok(result) => {
            let body = RunContainerResponse::from(result);
            (StatusCode::CREATED, Json(body)).into_response()
        }
        Err(response) => *response,
    }
}

fn validate_run_request(request: &RunContainerRequest) -> Result<()> {
    validate_image_reference(&request.image)?;
    validate_container_name(&request.name)?;
    if request.container_port == 0 {
        bail!("container port must be between 1 and 65535");
    }
    if request.host_port == Some(0) {
        bail!("host port must be between 1 and 65535");
    }
    Ok(())
}
