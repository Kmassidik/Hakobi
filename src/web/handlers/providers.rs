//! Provider connection handlers: list, add, test, toggle, delete.

use axum::{
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{
    html_response, internal_error, load_providers, page_csrf, post_is_allowed, redirect_response,
    truncate, with_catalog, CsrfForm, ProviderNotice,
};
use crate::web::{handlers::redirect_with_notice, templates, AppState};
use hakobi::providers::docker::validate_endpoint;

#[derive(Debug, Deserialize)]
pub struct ProviderForm {
    name: String,
    endpoint: String,
    #[serde(default)]
    secret: String,
    csrf_token: String,
}

pub(crate) async fn providers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let providers = match load_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::providers(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &providers,
            notice.notice.as_deref(),
        ),
        cookies,
    )
}

pub(crate) async fn add_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ProviderForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let name = form.name.trim();
    if name.is_empty()
        || name.len() > 64
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return (
            StatusCode::BAD_REQUEST,
            "provider name must be 1–64 ASCII letters, digits, dots, dashes, or underscores",
        )
            .into_response();
    }
    if let Err(error) = validate_endpoint(&form.endpoint) {
        return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
    }
    if form.secret.len() > 4096 {
        return (StatusCode::BAD_REQUEST, "provider secret is too long").into_response();
    }
    let name = name.to_owned();
    let endpoint = form.endpoint.trim().to_owned();
    let secret = form.secret;
    let result = with_catalog(&state, move |catalog| {
        catalog.create_provider_connection(&name, "docker", &endpoint, &secret)
    })
    .await;
    match result {
        Ok(_) => redirect_response("/providers?notice=Provider+added", Vec::new()),
        Err(error) => internal_error(error),
    }
}

pub(crate) async fn test_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let providers = match load_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let Some(provider) = providers.into_iter().find(|provider| provider.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let result = state.backend.test(&provider.endpoint).await;
    let error = result
        .as_ref()
        .err()
        .map(|error| truncate(&error.to_string(), 500));
    let id = provider.id;
    let record = with_catalog(&state, move |catalog| {
        catalog.record_provider_health(&id, error.as_deref())
    })
    .await;
    if let Err(error) = record {
        return internal_error(error);
    }
    let notice = if result.is_ok() {
        "Connection+OK"
    } else {
        "Connection+failed"
    };
    redirect_response(&format!("/providers?notice={notice}"), Vec::new())
}

pub(crate) async fn toggle_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let providers = match load_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let Some(provider) = providers.into_iter().find(|provider| provider.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let enabled = !provider.enabled;
    let id = provider.id;
    let result = with_catalog(&state, move |catalog| {
        catalog.set_provider_enabled(&id, enabled)
    })
    .await;
    match result {
        Ok(()) => redirect_response("/providers?notice=Provider+updated", Vec::new()),
        Err(error) => internal_error(error),
    }
}

pub(crate) async fn delete_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = with_catalog(&state, move |catalog| {
        catalog.delete_provider_connection(&id)
    })
    .await;
    match result {
        Ok(()) => redirect_with_notice("/providers", "Provider deleted."),
        Err(error) => internal_error(error),
    }
}
