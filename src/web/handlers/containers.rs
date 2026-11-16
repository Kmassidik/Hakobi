//! Container inventory, run form, and lifecycle action handlers.

use axum::{
    extract::{Form, Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{
    app_names_from_apps, container_catalog, html_response, internal_error, load_providers,
    page_csrf, post_is_allowed, truncate, CsrfForm,
};
use crate::web::{
    handlers::{redirect_with_error, redirect_with_notice},
    templates, AppState,
};
use hakobi::{
    catalog::ProviderConnection,
    providers::{
        docker::{validate_container_name, validate_image_reference},
        ContainerInventory,
    },
};

pub(crate) async fn containers(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let query = crate::web::pagination::parse_query(raw_query.as_deref());
    let (providers, apps) = match container_catalog(&state).await {
        Ok(catalog) => catalog,
        Err(error) => return internal_error(error),
    };
    let app_names = app_names_from_apps(&apps);
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let mut snapshots = std::collections::HashMap::new();
    let mut groups: Vec<(ProviderConnection, Result<ContainerInventory, String>)> = Vec::new();
    for provider in providers.into_iter().filter(|provider| provider.enabled) {
        let result = state
            .backend
            .containers(&provider.endpoint)
            .await
            .map(ContainerInventory::from_rows)
            .map_err(|error| truncate(&error.to_string(), 500));
        let snapshot = state
            .backend
            .snapshot(&provider.endpoint)
            .await
            .map_err(|error| truncate(&error.to_string(), 500));
        snapshots.insert(provider.id.clone(), snapshot);
        groups.push((provider, result));
    }
    html_response(
        StatusCode::OK,
        templates::containers(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &groups,
            &snapshots,
            &app_names,
            &apps,
            &query,
            query.get("notice").map(String::as_str),
        ),
        cookies,
    )
}

#[derive(Debug, Deserialize)]
pub struct ContainerForm {
    provider_id: String,
    image: String,
    name: String,
    container_port: String,
    csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct DeleteContainerForm {
    confirm_name: String,
    csrf_token: String,
}

#[derive(Debug, Clone, Copy)]
enum ContainerAction {
    Start,
    Stop,
    Restart,
}

impl ContainerAction {
    fn verb(self) -> &'static str {
        match self {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
        }
    }

    fn success_notice(self, name: &str) -> String {
        match self {
            ContainerAction::Start => format!("Container {name} started."),
            ContainerAction::Stop => format!("Container {name} stopped."),
            ContainerAction::Restart => format!("Container {name} restarted."),
        }
    }
}

fn render_container_form(
    state: &AppState,
    headers: &HeaderMap,
    providers: &[ProviderConnection],
    values: &templates::ContainerFormValues,
    error: Option<&str>,
    status: StatusCode,
) -> Response {
    let (csrf_token, cookies) = page_csrf(state, headers);
    html_response(
        status,
        templates::container_form(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            providers,
            values,
            error,
        ),
        cookies,
    )
}

async fn enabled_docker_providers(state: &AppState) -> anyhow::Result<Vec<ProviderConnection>> {
    let providers = load_providers(state).await?;
    Ok(providers
        .into_iter()
        .filter(|provider| provider.enabled && provider.kind == "docker")
        .collect())
}

pub(super) async fn enabled_docker_provider(
    state: &AppState,
    provider_id: &str,
) -> anyhow::Result<Option<ProviderConnection>> {
    Ok(enabled_docker_providers(state)
        .await?
        .into_iter()
        .find(|provider| provider.id == provider_id))
}

pub(crate) async fn container_new(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let providers = match enabled_docker_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let values = templates::ContainerFormValues {
        provider_id: providers
            .first()
            .map(|provider| provider.id.clone())
            .unwrap_or_default(),
        image: String::new(),
        name: String::new(),
        container_port: String::new(),
    };
    render_container_form(&state, &headers, &providers, &values, None, StatusCode::OK)
}

pub(crate) async fn container_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ContainerForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let providers = match enabled_docker_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let values = container_form_values(&form);
    let Some(provider) = providers
        .iter()
        .find(|provider| provider.id == values.provider_id)
    else {
        if values.provider_id.is_empty() {
            return render_container_form(
                &state,
                &headers,
                &providers,
                &values,
                Some("Select an enabled Docker provider."),
                StatusCode::BAD_REQUEST,
            );
        }
        return StatusCode::NOT_FOUND.into_response();
    };
    let container_port = match validate_container_values(&state, &headers, &providers, &values) {
        Ok(port) => port,
        Err(response) => return *response,
    };
    run_new_container(
        &state,
        &headers,
        provider,
        &providers,
        &values,
        container_port,
    )
    .await
}

fn container_form_values(form: &ContainerForm) -> templates::ContainerFormValues {
    templates::ContainerFormValues {
        provider_id: form.provider_id.trim().to_owned(),
        image: form.image.trim().to_owned(),
        name: form.name.trim().to_owned(),
        container_port: form.container_port.trim().to_owned(),
    }
}

fn validate_container_values(
    state: &AppState,
    headers: &HeaderMap,
    providers: &[ProviderConnection],
    values: &templates::ContainerFormValues,
) -> Result<u16, Box<Response>> {
    if let Err(error) = validate_image_reference(&values.image) {
        return Err(Box::new(render_container_form(
            state,
            headers,
            providers,
            values,
            Some(&error.to_string()),
            StatusCode::BAD_REQUEST,
        )));
    }
    if let Err(error) = validate_container_name(&values.name) {
        return Err(Box::new(render_container_form(
            state,
            headers,
            providers,
            values,
            Some(&error.to_string()),
            StatusCode::BAD_REQUEST,
        )));
    }
    match values.container_port.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(Box::new(render_container_form(
            state,
            headers,
            providers,
            values,
            Some("Container port must be a whole number between 1 and 65535."),
            StatusCode::BAD_REQUEST,
        ))),
    }
}

async fn run_new_container(
    state: &AppState,
    headers: &HeaderMap,
    provider: &ProviderConnection,
    providers: &[ProviderConnection],
    values: &templates::ContainerFormValues,
    container_port: u16,
) -> Response {
    let result = state
        .backend
        .run_container(
            &provider.endpoint,
            &values.image,
            &values.name,
            container_port,
            &std::collections::HashMap::new(),
            hakobi::providers::ResourceLimits::default(),
        )
        .await;
    match result {
        Ok(run) => {
            let notice = format!(
                "Container {} is running on host port {}.",
                run.name, run.host_port
            );
            redirect_with_notice("/containers", &notice)
        }
        Err(error) => {
            tracing::warn!(error = %error, "container run failed");
            render_container_form(
                state,
                headers,
                providers,
                values,
                Some(&truncate(&error.to_string(), 500)),
                StatusCode::BAD_GATEWAY,
            )
        }
    }
}

pub(crate) async fn container_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
    Form(form): Form<CsrfForm>,
) -> Response {
    container_action(
        &state,
        &headers,
        &provider,
        &name,
        &form.csrf_token,
        ContainerAction::Start,
    )
    .await
}

pub(crate) async fn container_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
    Form(form): Form<CsrfForm>,
) -> Response {
    container_action(
        &state,
        &headers,
        &provider,
        &name,
        &form.csrf_token,
        ContainerAction::Stop,
    )
    .await
}

pub(crate) async fn container_restart(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
    Form(form): Form<CsrfForm>,
) -> Response {
    container_action(
        &state,
        &headers,
        &provider,
        &name,
        &form.csrf_token,
        ContainerAction::Restart,
    )
    .await
}

pub(crate) async fn container_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
    Form(form): Form<DeleteContainerForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if templates::is_control_plane_container(&name) {
        return redirect_with_error(
            "/containers",
            "The control-plane container hakobi-z9pe cannot be removed from the workbench.",
        );
    }
    if form.confirm_name != name {
        return (
            StatusCode::BAD_REQUEST,
            "The typed name does not match this container. Nothing was removed.",
        )
            .into_response();
    }
    let provider = match enabled_docker_provider(&state, provider.as_str()).await {
        Ok(Some(provider)) => provider,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    if validate_container_name(&name).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let result = state
        .backend
        .remove_container(&provider.endpoint, &name)
        .await;
    match result {
        Ok(()) => redirect_with_notice("/containers", &format!("Container {name} removed.")),
        Err(error) => {
            tracing::warn!(error = %error, "container removal failed");
            let notice = format!(
                "Removing {name} failed: {}",
                truncate(&error.to_string(), 500)
            );
            redirect_with_error("/containers", &notice)
        }
    }
}

async fn container_action(
    state: &AppState,
    headers: &HeaderMap,
    provider_id: &str,
    name: &str,
    csrf_token: &str,
    action: ContainerAction,
) -> Response {
    if !super::authenticated(state, headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(state, headers, csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if validate_container_name(name).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if templates::is_control_plane_container(name) {
        let message = format!(
            "The workbench cannot {} the control-plane container hakobi-z9pe.",
            action.verb()
        );
        return redirect_with_error("/containers", &message);
    }
    let provider = match enabled_docker_provider(state, provider_id).await {
        Ok(Some(provider)) => provider,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    if let Err(message) = check_action_state(state, &provider.endpoint, name, action).await {
        return redirect_with_error("/containers", &message);
    }
    match execute_action(state, &provider.endpoint, name, action).await {
        Ok(()) => redirect_with_notice("/containers", &action.success_notice(name)),
        Err(error) => {
            tracing::warn!(error = %error, action = action.verb(), "container action failed");
            let message = format!(
                "Could not {} {name}: {}",
                action.verb(),
                truncate(&error.to_string(), 500)
            );
            redirect_with_error("/containers", &message)
        }
    }
}

async fn check_action_state(
    state: &AppState,
    endpoint: &str,
    name: &str,
    action: ContainerAction,
) -> Result<(), String> {
    let rows = state.backend.containers(endpoint).await.map_err(|error| {
        format!(
            "Could not check {name} before {}: {}",
            action.verb(),
            truncate(&error.to_string(), 500)
        )
    })?;
    let container = rows
        .iter()
        .find(|container| container.name == name)
        .ok_or_else(|| format!("Container {name} no longer exists on this provider."))?;
    if templates::container_action_allowed(action.verb(), &container.state) {
        Ok(())
    } else {
        Err(format!(
            "Cannot {} {name} while its state is {}.",
            action.verb(),
            container.state
        ))
    }
}

async fn execute_action(
    state: &AppState,
    endpoint: &str,
    name: &str,
    action: ContainerAction,
) -> anyhow::Result<()> {
    match action {
        ContainerAction::Start => state.backend.start_container(endpoint, name).await,
        ContainerAction::Stop => state.backend.stop_container(endpoint, name).await,
        ContainerAction::Restart => state.backend.restart_container(endpoint, name).await,
    }
}
