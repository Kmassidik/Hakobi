//! Shared plumbing for HTTP handlers: auth checks, CSRF, catalog access,
//! response helpers, and embedded assets.

mod auth;
mod containers;
mod dashboard;
mod deployment_pages;
mod environment_pages;
mod environments;
mod hosts;
mod infrastructure;
mod partials;
mod providers;

pub(crate) use auth::{login, logout};
pub(crate) use containers::{
    container_create, container_delete, container_new, container_restart, container_start,
    container_stop, containers,
};
pub(crate) use dashboard::dashboard;
pub(crate) use deployment_pages::{
    deployment_list_partial, deployment_page, environment_webhook_page, environment_webhook_submit,
};
pub(crate) use environment_pages::{
    environment_delete, environment_deploy, environment_deployments, environment_detail,
    environment_edit_page, environment_edit_submit, environment_logs, environment_rollback,
    environment_status_partial,
};
pub(crate) use environments::{environment_create, environment_list, environment_new};
pub(crate) use hosts::{
    host_history, host_resources, host_resources_partial, hosts_create, hosts_delete, hosts_list,
    hosts_test, hosts_toggle,
};
pub(crate) use infrastructure::{infrastructure, stack_detail, stacks};
pub(crate) use partials::{install_tool, metrics_partial, readiness_partial, status};
pub(crate) use providers::{
    add_provider, delete_provider, providers, test_provider, toggle_provider,
};

use std::{fmt::Write as _, sync::LazyLock};

use anyhow::Context;
use axum::{
    body::Body,
    extract::State,
    http::{
        header::{CACHE_CONTROL, LOCATION, ORIGIN, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Response},
};
use maud::Markup;
use rust_embed::RustEmbed;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{
    auth::{cookie_value, create_cookie, csrf_is_valid},
    templates, AppState,
};
use crate::{config::PublicConfig, utils::random::random_token};
use hakobi::catalog::Catalog;

pub(crate) const SESSION_COOKIE_AGE: u64 = 12 * 60 * 60;
pub(crate) const CSRF_COOKIE_AGE: u64 = 24 * 60 * 60;

#[derive(Debug, Deserialize)]
pub struct CsrfForm {
    csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ProviderNotice {
    pub notice: Option<String>,
}

pub(crate) async fn authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    let session = cookie_value(headers, state.config.session_cookie_name());
    state.sessions.is_valid(session).await
}

pub(crate) fn page_csrf(state: &AppState, headers: &HeaderMap) -> (String, Vec<String>) {
    match cookie_value(headers, state.config.csrf_cookie_name()) {
        Some(token) => (token.to_owned(), Vec::new()),
        None => {
            let token = random_token();
            let cookie = create_cookie(
                state.config.as_ref(),
                state.config.csrf_cookie_name(),
                &token,
                CSRF_COOKIE_AGE,
            );
            (token, vec![cookie])
        }
    }
}

pub(crate) fn post_is_allowed(state: &AppState, headers: &HeaderMap, token: &str) -> bool {
    origin_is_valid(headers, &state.config) && csrf_is_valid(headers, &state.config, token)
}

pub(crate) async fn load_providers(
    state: &AppState,
) -> anyhow::Result<Vec<hakobi::catalog::ProviderConnection>> {
    with_catalog(state, |catalog| catalog.list_provider_connections()).await
}

pub(crate) async fn with_catalog<T, F>(state: &AppState, operation: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Catalog) -> anyhow::Result<T> + Send + 'static,
{
    let path = (*state.catalog_path).clone();
    tokio::task::spawn_blocking(move || {
        let mut catalog = Catalog::open(path)?;
        operation(&mut catalog)
    })
    .await
    .context("catalog task failed")?
}

pub(crate) fn truncate(value: &str, limit: usize) -> String {
    let mut result: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        result.push('…');
    }
    result
}

pub(crate) fn internal_error(error: anyhow::Error) -> Response {
    tracing::error!(error = %error, "request failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
}

pub(crate) fn html_response(status: StatusCode, markup: Markup, cookies: Vec<String>) -> Response {
    let mut response = (status, Html(markup.into_string())).into_response();
    set_cookies(&mut response, cookies);
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(crate) fn redirect_response(location: &str, cookies: Vec<String>) -> Response {
    let mut response = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(LOCATION, location)
        .header(CACHE_CONTROL, "no-store")
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    set_cookies(&mut response, cookies);
    response
}

fn set_cookies(response: &mut Response, cookies: Vec<String>) {
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(SET_COOKIE, value);
        }
    }
}

pub(crate) fn origin_is_valid(headers: &HeaderMap, config: &PublicConfig) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| origin == "null" || origin == config.base_url)
}

pub(crate) fn login_key(headers: &HeaderMap) -> String {
    headers
        .get("cf-connecting-ip")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.parse::<std::net::IpAddr>().is_ok())
        .unwrap_or("local")
        .to_owned()
}

pub(crate) fn redirect_with_notice(base: &str, message: &str) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(message.as_bytes()).collect();
    redirect_response(&format!("{base}?notice={encoded}"), Vec::new())
}

#[derive(RustEmbed)]
#[folder = "static/"]
struct Assets;

static ASSET_VERSION: LazyLock<String> = LazyLock::new(|| {
    let mut hasher = Sha256::new();
    for asset in [
        "assets/app.css",
        "assets/hakobi.js",
        "assets/htmx.min.js",
        "assets/vendor/fontawesome/css/all.min.css",
        "assets/vendor/fontawesome/webfonts/fa-solid-900.woff2",
    ] {
        if let Some(content) = Assets::get(asset) {
            hasher.update(content.data);
        }
    }
    let digest = hasher.finalize();
    let mut version = String::with_capacity(12);
    for byte in digest.iter().take(6) {
        let _ = write!(version, "{byte:02x}");
    }
    version
});

pub(crate) fn asset_url(path: &str) -> String {
    format!("/assets/{path}?v={}", *ASSET_VERSION)
}

pub(crate) async fn asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    let path = format!("assets/{}", path.trim_start_matches('/'));
    let Some(content) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    (
        [
            (axum::http::header::CONTENT_TYPE, mime.to_string()),
            (CACHE_CONTROL, "public, max-age=3600".into()),
        ],
        content.data.into_owned(),
    )
        .into_response()
}

/// Renders the login page; kept in mod.rs since it is shared by auth routes.
pub(crate) async fn login_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = cookie_value(&headers, state.config.session_cookie_name());
    if state.sessions.is_valid(session).await {
        return axum::response::Redirect::to("/").into_response();
    }
    render_login(&state, None, String::new(), StatusCode::OK, Vec::new())
}

pub(crate) fn render_login(
    state: &AppState,
    error: Option<String>,
    username: String,
    status: StatusCode,
    mut cookies: Vec<String>,
) -> Response {
    let csrf_token = random_token();
    let markup = templates::login(
        &state.config.base_url,
        &csrf_token,
        error.as_deref(),
        &username,
    );
    cookies.push(create_cookie(
        &state.config,
        state.config.csrf_cookie_name(),
        &csrf_token,
        CSRF_COOKIE_AGE,
    ));
    html_response(status, markup, cookies)
}
