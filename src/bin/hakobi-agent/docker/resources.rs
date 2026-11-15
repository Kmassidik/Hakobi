//! Native Docker image, network, and volume mutations.

use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::{
    host_agent::{self, DockerResourceRequest, RemoveImageRequest},
    providers::{
        validate_container_name, validate_image_reference, ContainerBackend, DockerBackend,
    },
};

use super::super::{authorized, error_response, unauthorized, AgentState};
use super::{docker_call, DOCKER_TIMEOUT};

pub(crate) async fn docker_remove_image(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<RemoveImageRequest>, JsonRejection>,
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
    if let Err(error) = validate_image_reference(&request.image) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let backend = DockerBackend;
    match docker_call(
        backend.remove_image(host_agent::DOCKER_SOCKET_ENDPOINT, &request.image),
        DOCKER_TIMEOUT,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => *response,
    }
}

#[derive(Clone, Copy)]
enum ResourceAction {
    CreateNetwork,
    DeleteNetwork,
    CreateVolume,
    DeleteVolume,
}

async fn resource_action(
    state: AgentState,
    headers: HeaderMap,
    payload: std::result::Result<Json<DockerResourceRequest>, JsonRejection>,
    action: ResourceAction,
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
    if let Err(error) = validate_container_name(&request.name) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    if matches!(action, ResourceAction::DeleteNetwork)
        && matches!(request.name.as_str(), "bridge" | "host" | "none")
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            format!("built-in network `{}` cannot be removed", request.name),
        );
    }
    let backend = DockerBackend;
    let endpoint = host_agent::DOCKER_SOCKET_ENDPOINT;
    let name = &request.name;
    let result = match action {
        ResourceAction::CreateNetwork => {
            docker_call(backend.create_network(endpoint, name), DOCKER_TIMEOUT).await
        }
        ResourceAction::DeleteNetwork => {
            docker_call(backend.remove_network(endpoint, name), DOCKER_TIMEOUT).await
        }
        ResourceAction::CreateVolume => {
            docker_call(backend.create_volume(endpoint, name), DOCKER_TIMEOUT).await
        }
        ResourceAction::DeleteVolume => {
            docker_call(backend.remove_volume(endpoint, name), DOCKER_TIMEOUT).await
        }
    };
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => *response,
    }
}

pub(crate) async fn docker_create_network(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<DockerResourceRequest>, JsonRejection>,
) -> Response {
    resource_action(state, headers, payload, ResourceAction::CreateNetwork).await
}

pub(crate) async fn docker_delete_network(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<DockerResourceRequest>, JsonRejection>,
) -> Response {
    resource_action(state, headers, payload, ResourceAction::DeleteNetwork).await
}

pub(crate) async fn docker_create_volume(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<DockerResourceRequest>, JsonRejection>,
) -> Response {
    resource_action(state, headers, payload, ResourceAction::CreateVolume).await
}

pub(crate) async fn docker_delete_volume(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<DockerResourceRequest>, JsonRejection>,
) -> Response {
    resource_action(state, headers, payload, ResourceAction::DeleteVolume).await
}
