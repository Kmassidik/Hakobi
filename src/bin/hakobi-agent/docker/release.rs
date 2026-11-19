//! Authenticated one-shot release commands with captured output.

use std::time::Duration;

use anyhow::{bail, Result};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::{
    host_agent::{self, ReleaseContainerRequest, ReleaseContainerResponse},
    providers::{
        validate_container_name, validate_image_reference, validate_shell_command,
        validate_volumes, ContainerBackend, DockerBackend,
    },
};

use super::super::{authorized, error_response, unauthorized, AgentState};
use super::docker_call;

const DOCKER_RELEASE_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) async fn docker_release(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<ReleaseContainerRequest>, JsonRejection>,
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
    if let Err(error) = validate_release_request(&request) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let operation = DockerBackend.release_container(
        host_agent::DOCKER_SOCKET_ENDPOINT,
        &request.image,
        &request.name,
        &request.env,
        &request.volumes,
        &request.command,
    );
    match docker_call(operation, DOCKER_RELEASE_TIMEOUT).await {
        Ok(result) => (
            StatusCode::OK,
            Json(ReleaseContainerResponse {
                exit_code: result.exit_code,
                logs: result.logs,
            }),
        )
            .into_response(),
        Err(response) => *response,
    }
}

fn validate_release_request(request: &ReleaseContainerRequest) -> Result<()> {
    validate_image_reference(&request.image)?;
    validate_container_name(&request.name)?;
    validate_volumes(&request.volumes)?;
    validate_shell_command(&request.command)?;
    if request.command.is_empty() {
        bail!("release command must not be empty");
    }
    Ok(())
}
