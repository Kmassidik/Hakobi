//! Infrastructure snapshot and runtime stack detail handlers.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::{
    html_response, internal_error, load_providers, page_csrf, truncate, with_catalog,
    ProviderNotice,
};
use crate::web::{templates, AppState};
use hakobi::{
    catalog::ProviderConnection,
    providers::{ContainerInventory, DockerSnapshot},
};

pub(crate) async fn infrastructure(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let providers = match load_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let mut groups: Vec<(ProviderConnection, Result<DockerSnapshot, String>)> = Vec::new();
    for provider in providers.into_iter().filter(|provider| provider.enabled) {
        let result = state
            .backend
            .snapshot(&provider.endpoint)
            .await
            .map_err(|error| truncate(&error.to_string(), 500));
        groups.push((provider, result));
    }
    html_response(
        StatusCode::OK,
        templates::infrastructure(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &groups,
        ),
        cookies,
    )
}

pub(crate) async fn stack_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let providers = match load_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let Some(provider) = providers
        .into_iter()
        .find(|p| p.id == provider && p.enabled)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let rows = match state.backend.containers(&provider.endpoint).await {
        Ok(rows) => rows,
        Err(error) => {
            return html_response(
                StatusCode::BAD_GATEWAY,
                templates::stack_detail(
                    &state.config.base_url,
                    &state.config.username,
                    &csrf_token,
                    &provider,
                    &name,
                    Err(truncate(&error.to_string(), 500)),
                ),
                cookies,
            );
        }
    };
    let inventory = ContainerInventory::from_rows(rows);
    let stack = inventory
        .stacks
        .into_iter()
        .find(|stack| stack.name == name);
    match stack {
        Some(stack) => {
            let stack_name = stack.name.clone();
            html_response(
                StatusCode::OK,
                templates::stack_detail(
                    &state.config.base_url,
                    &state.config.username,
                    &csrf_token,
                    &provider,
                    &stack_name,
                    Ok(stack),
                ),
                cookies,
            )
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) async fn stacks(
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
    let managed = match with_catalog(&state, |catalog| catalog.list_stacks()).await {
        Ok(stacks) => stacks,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let mut groups: Vec<(ProviderConnection, Result<ContainerInventory, String>)> = Vec::new();
    for provider in providers
        .iter()
        .filter(|provider| provider.enabled)
        .cloned()
    {
        let result = state
            .backend
            .containers(&provider.endpoint)
            .await
            .map(ContainerInventory::from_rows)
            .map_err(|error| truncate(&error.to_string(), 500));
        groups.push((provider, result));
    }
    html_response(
        StatusCode::OK,
        templates::stacks(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &groups,
            &providers,
            &managed,
            notice.notice.as_deref(),
        ),
        cookies,
    )
}
