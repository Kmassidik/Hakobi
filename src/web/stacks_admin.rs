//! Managed stack definition CRUD.
//!
//! Every route here mutates or reads catalog metadata only. Nothing is
//! deployed: apply, build, and Kubernetes execution are deliberately absent.

use std::str::FromStr;

use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use super::{
    handlers::{
        authenticated, html_response, internal_error, page_csrf, post_is_allowed, truncate,
        with_catalog,
    },
    templates::{self, StackFormValues},
    AppState,
};
use crate::web::handlers::load_providers;
use hakobi::{
    catalog::ProviderConnection,
    stacks::{StackDefinition, StackKind},
};

#[derive(Debug, Deserialize)]
pub(super) struct StackForm {
    kind: String,
    #[serde(default)]
    provider_id: String,
    name: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    yaml: String,
    csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct DeleteStackForm {
    #[serde(default)]
    confirm_name: String,
    csrf_token: String,
}

pub(super) async fn stack_new(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    let providers = match enabled_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let values = StackFormValues {
        kind: StackKind::Compose.as_str().to_owned(),
        provider_id: providers
            .first()
            .map(|provider| provider.id.clone())
            .unwrap_or_default(),
        name: String::new(),
        source: String::new(),
        yaml: String::new(),
    };
    render_form(
        &state,
        &headers,
        &providers,
        &values,
        "/stacks/new",
        "New stack",
        "Create stack",
        None,
        None,
        StatusCode::OK,
    )
}

pub(super) async fn stack_create(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Form(form): Form<StackForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let providers = match enabled_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let values = StackFormValues {
        kind: form.kind.trim().to_owned(),
        provider_id: form.provider_id.trim().to_owned(),
        name: form.name.trim().to_owned(),
        source: form.source.trim().to_owned(),
        yaml: form.yaml,
    };
    let (kind, provider_id) = match validate_form(&values, &providers) {
        Ok(parsed) => parsed,
        Err(message) => {
            return render_form(
                &state,
                &headers,
                &providers,
                &values,
                "/stacks/new",
                "New stack",
                "Create stack",
                None,
                Some(&message),
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let name = values.name.clone();
    let source = values.source.clone();
    let yaml = values.yaml.clone();
    let result = with_catalog(&state, move |catalog| {
        catalog
            .create_stack(&provider_id, &name, kind, &source, &yaml)
            .map(|_| ())
    })
    .await;
    match result {
        Ok(()) => super::handlers::redirect_response("/stacks?notice=Stack+created", Vec::new()),
        Err(error) => render_form(
            &state,
            &headers,
            &providers,
            &values,
            "/stacks/new",
            "New stack",
            "Create stack",
            None,
            Some(&display_error(&error)),
            StatusCode::BAD_REQUEST,
        ),
    }
}

pub(super) async fn stack_edit(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    let stack = match load_stack(&state, &id).await {
        Ok(Some(stack)) => stack,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let providers = match enabled_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let provider_name = provider_label(&providers, &stack.provider_id);
    let values = StackFormValues::from_definition(&stack);
    render_form(
        &state,
        &headers,
        &providers,
        &values,
        &format!("/stacks/{id}/edit"),
        "Edit stack",
        "Save changes",
        Some(&provider_name),
        None,
        StatusCode::OK,
    )
}

pub(super) async fn stack_update(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<StackForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let stack = match load_stack(&state, &id).await {
        Ok(Some(stack)) => stack,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let providers = match enabled_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let provider_name = provider_label(&providers, &stack.provider_id);
    let values = StackFormValues {
        kind: form.kind.trim().to_owned(),
        provider_id: stack.provider_id.clone(),
        name: form.name.trim().to_owned(),
        source: form.source.trim().to_owned(),
        yaml: form.yaml,
    };
    let kind = match validate_form_without_provider(&values) {
        Ok(kind) => kind,
        Err(message) => {
            return render_form(
                &state,
                &headers,
                &providers,
                &values,
                &format!("/stacks/{id}/edit"),
                "Edit stack",
                "Save changes",
                Some(&provider_name),
                Some(&message),
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let name = values.name.clone();
    let source = values.source.clone();
    let yaml = values.yaml.clone();
    let stack_id = stack.id.clone();
    let result = with_catalog(&state, move |catalog| {
        catalog.update_stack(&stack_id, &name, kind, &source, &yaml)
    })
    .await;
    match result {
        Ok(()) => super::handlers::redirect_response("/stacks?notice=Stack+updated", Vec::new()),
        Err(error) => render_form(
            &state,
            &headers,
            &providers,
            &values,
            &format!("/stacks/{id}/edit"),
            "Edit stack",
            "Save changes",
            Some(&provider_name),
            Some(&display_error(&error)),
            StatusCode::BAD_REQUEST,
        ),
    }
}

pub(super) async fn stack_delete(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<DeleteStackForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let stack = match load_stack(&state, &id).await {
        Ok(Some(stack)) => stack,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    if form.confirm_name != stack.name {
        let providers = match enabled_providers(&state).await {
            Ok(providers) => providers,
            Err(error) => return internal_error(error),
        };
        let provider_name = provider_label(&providers, &stack.provider_id);
        return render_delete_page(
            &state,
            &headers,
            &stack,
            &provider_name,
            Some("The typed name does not match this stack. Nothing was deleted."),
            StatusCode::BAD_REQUEST,
        );
    }
    let stack_id = stack.id.clone();
    let result = with_catalog(&state, move |catalog| catalog.delete_stack(&stack_id)).await;
    match result {
        Ok(()) => super::handlers::redirect_response("/stacks?notice=Stack+deleted", Vec::new()),
        Err(error) => internal_error(error),
    }
}

impl StackFormValues {
    fn from_definition(stack: &StackDefinition) -> Self {
        Self {
            kind: stack.kind.as_str().to_owned(),
            provider_id: stack.provider_id.clone(),
            name: stack.name.clone(),
            source: stack.source.clone(),
            yaml: stack.yaml.clone(),
        }
    }
}

async fn load_stack(state: &AppState, id: &str) -> anyhow::Result<Option<StackDefinition>> {
    let id = id.to_owned();
    with_catalog(state, move |catalog| catalog.get_stack(&id)).await
}

async fn enabled_providers(state: &AppState) -> anyhow::Result<Vec<ProviderConnection>> {
    let providers = load_providers(state).await?;
    Ok(providers
        .into_iter()
        .filter(|provider| provider.enabled)
        .collect())
}

fn provider_label(providers: &[ProviderConnection], provider_id: &str) -> String {
    providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .map(|provider| format!("{} · {}", provider.name, provider.endpoint))
        .unwrap_or_else(|| provider_id.to_owned())
}

fn validate_form(
    values: &StackFormValues,
    providers: &[ProviderConnection],
) -> Result<(StackKind, String), String> {
    let kind = validate_form_without_provider(values)?;
    if values.provider_id.is_empty() {
        return Err("Choose an enabled provider for this stack.".into());
    }
    if !providers
        .iter()
        .any(|provider| provider.id == values.provider_id)
    {
        return Err("The selected provider is missing or disabled.".into());
    }
    Ok((kind, values.provider_id.clone()))
}

fn validate_form_without_provider(values: &StackFormValues) -> Result<StackKind, String> {
    let kind = StackKind::from_str(&values.kind).map_err(|_| {
        "Choose a supported stack kind: dockerfile, compose, kubernetes, or nixpacks.".to_owned()
    })?;
    if values.name.is_empty()
        || values.name.len() > 64
        || !values.name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(
            "Stack name must be 1–64 ASCII letters, digits, dots, dashes, or underscores.".into(),
        );
    }
    if values.source.is_empty() || values.source.len() > 1024 {
        return Err("Source must be 1–1024 characters.".into());
    }
    Ok(kind)
}

fn display_error(error: &anyhow::Error) -> String {
    truncate(&error.to_string(), 500)
}

#[allow(clippy::too_many_arguments)]
fn render_form(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    providers: &[ProviderConnection],
    values: &StackFormValues,
    action: &str,
    heading: &str,
    submit_label: &str,
    provider_name: Option<&str>,
    error: Option<&str>,
    status: StatusCode,
) -> Response {
    let (csrf_token, cookies) = page_csrf(state, headers);
    html_response(
        status,
        templates::stack_form(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            providers,
            values,
            action,
            heading,
            submit_label,
            provider_name,
            error,
        ),
        cookies,
    )
}

fn render_delete_page(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    stack: &StackDefinition,
    provider_name: &str,
    error: Option<&str>,
    status: StatusCode,
) -> Response {
    let (csrf_token, cookies) = page_csrf(state, headers);
    html_response(
        status,
        templates::stack_delete(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            stack,
            provider_name,
            error,
        ),
        cookies,
    )
}
