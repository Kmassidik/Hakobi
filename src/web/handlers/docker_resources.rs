//! Guarded Docker image, network, and volume mutations.

use axum::{
    extract::{Form, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::{internal_error, post_is_allowed, redirect_with_notice, AppState};
use crate::web::handlers::containers::enabled_docker_provider;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ResourceForm {
    provider_id: String,
    name: String,
    #[serde(default)]
    confirm_name: String,
    csrf_token: String,
}

#[derive(Clone, Copy)]
enum ResourceAction {
    ImageDelete,
    NetworkCreate,
    NetworkDelete,
    VolumeCreate,
    VolumeDelete,
}

impl ResourceAction {
    fn is_create(self) -> bool {
        matches!(self, Self::NetworkCreate | Self::VolumeCreate)
    }

    fn notice(self, name: &str) -> String {
        match self {
            Self::ImageDelete => format!("Image {name} deleted."),
            Self::NetworkCreate => format!("Network {name} created."),
            Self::NetworkDelete => format!("Network {name} deleted."),
            Self::VolumeCreate => format!("Volume {name} created."),
            Self::VolumeDelete => format!("Volume {name} deleted."),
        }
    }
}

pub(crate) async fn image_delete(
    state: State<AppState>,
    headers: HeaderMap,
    form: Form<ResourceForm>,
) -> Response {
    perform(state, headers, form, ResourceAction::ImageDelete).await
}

pub(crate) async fn network_create(
    state: State<AppState>,
    headers: HeaderMap,
    form: Form<ResourceForm>,
) -> Response {
    perform(state, headers, form, ResourceAction::NetworkCreate).await
}

pub(crate) async fn network_delete(
    state: State<AppState>,
    headers: HeaderMap,
    form: Form<ResourceForm>,
) -> Response {
    perform(state, headers, form, ResourceAction::NetworkDelete).await
}

pub(crate) async fn volume_create(
    state: State<AppState>,
    headers: HeaderMap,
    form: Form<ResourceForm>,
) -> Response {
    perform(state, headers, form, ResourceAction::VolumeCreate).await
}

pub(crate) async fn volume_delete(
    state: State<AppState>,
    headers: HeaderMap,
    form: Form<ResourceForm>,
) -> Response {
    perform(state, headers, form, ResourceAction::VolumeDelete).await
}

async fn perform(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ResourceForm>,
    action: ResourceAction,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !action.is_create() && form.name != form.confirm_name {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let provider = match enabled_docker_provider(&state, &form.provider_id).await {
        Ok(Some(provider)) => provider,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let name = form.name.trim();
    let result = execute(&state, &provider.endpoint, name, action).await;
    if let Err(error) = result {
        return internal_error(error);
    }
    let tab = match action {
        ResourceAction::ImageDelete => "images",
        ResourceAction::NetworkCreate | ResourceAction::NetworkDelete => "networks",
        ResourceAction::VolumeCreate | ResourceAction::VolumeDelete => "volumes",
    };
    redirect_with_notice(&format!("/containers?tab={tab}"), &action.notice(name))
}

async fn execute(
    state: &AppState,
    endpoint: &str,
    name: &str,
    action: ResourceAction,
) -> anyhow::Result<()> {
    match action {
        ResourceAction::ImageDelete => state.backend.remove_image(endpoint, name).await,
        ResourceAction::NetworkCreate => state.backend.create_network(endpoint, name).await,
        ResourceAction::NetworkDelete => state.backend.remove_network(endpoint, name).await,
        ResourceAction::VolumeCreate => state.backend.create_volume(endpoint, name).await,
        ResourceAction::VolumeDelete => state.backend.remove_volume(endpoint, name).await,
    }
}
