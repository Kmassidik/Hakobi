use std::{collections::HashSet, fmt::Write as _, fs, sync::LazyLock};

use anyhow::Context;
use axum::{
    body::Body,
    extract::{Form, Path, Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE, LOCATION, ORIGIN, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
};
use maud::Markup;
use rust_embed::RustEmbed;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::{
    auth::{self, clear_cookie, cookie_value, create_cookie, csrf_is_valid},
    templates, AppState,
};
use crate::{config::PublicConfig, utils::random::random_token};
use hakobi::{
    catalog::{Catalog, ProviderConnection},
    providers::{
        docker::{validate_container_name, validate_endpoint, validate_image_reference},
        ContainerInventory, DockerSnapshot,
    },
};

const SESSION_COOKIE_AGE: u64 = 12 * 60 * 60;
const CSRF_COOKIE_AGE: u64 = 24 * 60 * 60;

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct CsrfForm {
    csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct ProviderForm {
    name: String,
    endpoint: String,
    #[serde(default)]
    secret: String,
    csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct ProviderNotice {
    notice: Option<String>,
}

pub(super) async fn dashboard(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = cookie_value(&headers, state.config.session_cookie_name());
    if !state.sessions.is_valid(session).await {
        return Redirect::to("/login").into_response();
    }

    let existing_csrf = cookie_value(&headers, state.config.csrf_cookie_name());
    let csrf_token = existing_csrf
        .map(str::to_owned)
        .unwrap_or_else(random_token);
    let readiness = match tokio::task::spawn_blocking(hakobi::install::detect).await {
        Ok(readiness) => readiness,
        Err(error) => {
            tracing::error!(error = %error, "readiness detection failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let mut metrics = templates::DashboardMetrics::default();
    match load_providers(&state).await {
        Ok(providers) => {
            let mut services = HashSet::new();
            let mut running_services = HashSet::new();
            let mut stacks = HashSet::new();
            for provider in providers.into_iter().filter(|provider| provider.enabled) {
                let rows = match state.backend.containers(&provider.endpoint).await {
                    Ok(rows) => rows,
                    Err(error) => {
                        tracing::warn!(
                            provider = %provider.name,
                            error = %error,
                            "dashboard provider inventory failed"
                        );
                        continue;
                    }
                };
                for row in rows {
                    let running = row.state == "running";
                    metrics.containers_total += 1;
                    if running {
                        metrics.containers_running += 1;
                        if row.health.as_deref() != Some("unhealthy") {
                            metrics.containers_healthy += 1;
                        }
                    }
                    if row.state == "restarting" {
                        metrics.containers_restarting += 1;
                    }
                    if let Some(project) = row
                        .compose_project
                        .as_deref()
                        .filter(|value| !value.is_empty())
                    {
                        stacks.insert(project.to_owned());
                    }
                    let service = match (
                        row.compose_project.as_deref(),
                        row.compose_service.as_deref(),
                    ) {
                        (Some(project), Some(service)) => format!("{project}/{service}"),
                        _ => row.name.clone(),
                    };
                    services.insert(service.clone());
                    if running {
                        running_services.insert(service);
                    }
                }
            }
            metrics.services_total = services.len();
            metrics.services_running = running_services.len();
            metrics.stacks = stacks.len();
        }
        Err(error) => {
            tracing::warn!(error = %error, "dashboard provider list failed");
        }
    }
    if let Some((used, total)) = host_memory() {
        metrics.memory_used_bytes = Some(used);
        metrics.memory_total_bytes = Some(total);
    }
    if let Some(cpu_count) = std::thread::available_parallelism()
        .ok()
        .map(|value| value.get())
    {
        metrics.cpu_count = Some(cpu_count);
        if let Some(load) = host_load_average() {
            metrics.cpu_percent = Some(
                ((load / cpu_count as f32) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8,
            );
        }
    }
    let markup = templates::dashboard(
        &state.config.base_url,
        &csrf_token,
        state.config.tunnel_enabled,
        &state.config.username,
        &readiness,
        &metrics,
    );
    let cookies = if existing_csrf.is_none() {
        vec![create_cookie(
            &state.config,
            state.config.csrf_cookie_name(),
            &csrf_token,
            CSRF_COOKIE_AGE,
        )]
    } else {
        Vec::new()
    };
    html_response(StatusCode::OK, markup, cookies)
}

pub(super) async fn login_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = cookie_value(&headers, state.config.session_cookie_name());
    if state.sessions.is_valid(session).await {
        return Redirect::to("/").into_response();
    }
    render_login(&state, None, String::new(), StatusCode::OK, Vec::new())
}

pub(super) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let origin_valid = origin_is_valid(&headers, &state.config);
    let csrf_valid = csrf_is_valid(&headers, &state.config, &form.csrf_token);
    tracing::debug!(
        origin_valid,
        csrf_valid,
        origin = ?headers.get(ORIGIN),
        csrf_cookie_present = cookie_value(&headers, state.config.csrf_cookie_name()).is_some(),
        submitted_csrf_length = form.csrf_token.len(),
        "login request validation"
    );
    if !origin_valid || !csrf_valid {
        return render_login(
            &state,
            Some("Your login session expired. Please try again.".into()),
            form.username,
            StatusCode::BAD_REQUEST,
            Vec::new(),
        );
    }
    if form.username.len() > 64 || form.password.len() > 128 {
        return render_login(
            &state,
            Some("Invalid username or password.".into()),
            form.username,
            StatusCode::UNAUTHORIZED,
            Vec::new(),
        );
    }

    let limiter_key = login_key(&headers);
    if !state.limiter.allows(&limiter_key).await {
        return render_login(
            &state,
            Some("Too many attempts. Wait five minutes and try again.".into()),
            form.username,
            StatusCode::TOO_MANY_REQUESTS,
            Vec::new(),
        );
    }

    let Ok(_password_permit) = state.password_work.clone().try_acquire_owned() else {
        return render_login(
            &state,
            Some("Sign-in is busy. Try again shortly.".into()),
            form.username,
            StatusCode::TOO_MANY_REQUESTS,
            Vec::new(),
        );
    };

    let password = Zeroizing::new(form.password);
    let password_hash = state.config.password_hash.clone();
    let password_matches =
        tokio::task::spawn_blocking(move || auth::verify_password(&password_hash, &password))
            .await
            .unwrap_or(false);
    let username_matches = form.username == state.config.username;

    if !password_matches || !username_matches {
        state.limiter.record_failure(&limiter_key).await;
        return render_login(
            &state,
            Some("Invalid username or password.".into()),
            form.username,
            StatusCode::UNAUTHORIZED,
            Vec::new(),
        );
    }

    state.limiter.clear(&limiter_key).await;
    let old_session = cookie_value(&headers, state.config.session_cookie_name());
    state.sessions.revoke(old_session).await;
    let session = state.sessions.create().await;
    let csrf = random_token();
    let cookies = vec![
        create_cookie(
            &state.config,
            state.config.session_cookie_name(),
            &session,
            SESSION_COOKIE_AGE,
        ),
        create_cookie(
            &state.config,
            state.config.csrf_cookie_name(),
            &csrf,
            CSRF_COOKIE_AGE,
        ),
    ];
    redirect_response("/", cookies)
}

pub(super) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let session = cookie_value(&headers, state.config.session_cookie_name());
    if !state.sessions.is_valid(session).await {
        return Redirect::to("/login").into_response();
    }
    if !origin_is_valid(&headers, &state.config)
        || !csrf_is_valid(&headers, &state.config, &form.csrf_token)
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    state.sessions.revoke(session).await;
    redirect_response(
        "/login",
        vec![
            clear_cookie(&state.config, state.config.session_cookie_name()),
            clear_cookie(&state.config, state.config.csrf_cookie_name()),
        ],
    )
}

pub(super) async fn providers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn add_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ProviderForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn test_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn toggle_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn delete_provider(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = with_catalog(&state, move |catalog| {
        catalog.delete_provider_connection(&id)
    })
    .await;
    match result {
        Ok(()) => redirect_response("/providers?notice=Provider+deleted", Vec::new()),
        Err(error) => internal_error(error),
    }
}

pub(super) async fn containers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    let providers = match load_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let mut groups: Vec<(ProviderConnection, Result<ContainerInventory, String>)> = Vec::new();
    for provider in providers.into_iter().filter(|provider| provider.enabled) {
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
        templates::containers(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &groups,
            notice.notice.as_deref(),
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

pub(super) async fn container_new(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn container_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<ContainerForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let providers = match enabled_docker_providers(&state).await {
        Ok(providers) => providers,
        Err(error) => return internal_error(error),
    };
    let values = templates::ContainerFormValues {
        provider_id: form.provider_id.trim().to_owned(),
        image: form.image.trim().to_owned(),
        name: form.name.trim().to_owned(),
        container_port: form.container_port.trim().to_owned(),
    };
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
    if let Err(error) = validate_image_reference(&values.image) {
        return render_container_form(
            &state,
            &headers,
            &providers,
            &values,
            Some(&error.to_string()),
            StatusCode::BAD_REQUEST,
        );
    }
    if let Err(error) = validate_container_name(&values.name) {
        return render_container_form(
            &state,
            &headers,
            &providers,
            &values,
            Some(&error.to_string()),
            StatusCode::BAD_REQUEST,
        );
    }
    let container_port = match values.container_port.parse::<u16>() {
        Ok(port) if port > 0 => port,
        _ => {
            return render_container_form(
                &state,
                &headers,
                &providers,
                &values,
                Some("Container port must be a whole number between 1 and 65535."),
                StatusCode::BAD_REQUEST,
            );
        }
    };

    let result = state
        .backend
        .run_container(
            &provider.endpoint,
            &values.image,
            &values.name,
            container_port,
        )
        .await;
    match result {
        Ok(run) => {
            let notice = format!(
                "Container {} is running on host port {}.",
                run.name, run.host_port
            );
            redirect_response(&containers_notice(&notice), Vec::new())
        }
        Err(error) => {
            tracing::warn!(error = %error, "container run failed");
            render_container_form(
                &state,
                &headers,
                &providers,
                &values,
                Some(&truncate(&error.to_string(), 500)),
                StatusCode::BAD_GATEWAY,
            )
        }
    }
}

pub(super) async fn container_start(
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

pub(super) async fn container_stop(
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

pub(super) async fn container_restart(
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

pub(super) async fn container_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
    Form(form): Form<DeleteContainerForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
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
        Ok(()) => {
            let notice = format!("Container {name} removed.");
            redirect_response(&containers_notice(&notice), Vec::new())
        }
        Err(error) => {
            tracing::warn!(error = %error, "container removal failed");
            let notice = format!(
                "Removing {name} failed: {}",
                truncate(&error.to_string(), 500)
            );
            redirect_response(&containers_notice(&notice), Vec::new())
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
    if !authenticated(state, headers).await {
        return Redirect::to("/login").into_response();
    }
    if !post_is_allowed(state, headers, csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if validate_container_name(name).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let provider = match enabled_docker_provider(state, provider_id).await {
        Ok(Some(provider)) => provider,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let result = match action {
        ContainerAction::Start => {
            state
                .backend
                .start_container(&provider.endpoint, name)
                .await
        }
        ContainerAction::Stop => state.backend.stop_container(&provider.endpoint, name).await,
        ContainerAction::Restart => {
            state
                .backend
                .restart_container(&provider.endpoint, name)
                .await
        }
    };
    let notice = match result {
        Ok(()) => action.success_notice(name),
        Err(error) => {
            tracing::warn!(error = %error, action = action.verb(), "container action failed");
            format!(
                "Could not {} {name}: {}",
                action.verb(),
                truncate(&error.to_string(), 500)
            )
        }
    };
    redirect_response(&containers_notice(&notice), Vec::new())
}

async fn enabled_docker_providers(state: &AppState) -> anyhow::Result<Vec<ProviderConnection>> {
    let providers = load_providers(state).await?;
    Ok(providers
        .into_iter()
        .filter(|provider| provider.enabled && provider.kind == "docker")
        .collect())
}

async fn enabled_docker_provider(
    state: &AppState,
    provider_id: &str,
) -> anyhow::Result<Option<ProviderConnection>> {
    Ok(enabled_docker_providers(state)
        .await?
        .into_iter()
        .find(|provider| provider.id == provider_id))
}

fn host_memory() -> Option<(u64, u64)> {
    let contents = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kib = None;
    let mut available_kib = None;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            total_kib = first_u64(value);
        } else if let Some(value) = line.strip_prefix("MemAvailable:") {
            available_kib = first_u64(value);
        }
    }
    let total = total_kib?.saturating_mul(1024);
    let available = available_kib?.saturating_mul(1024);
    Some((total.saturating_sub(available), total))
}

fn first_u64(value: &str) -> Option<u64> {
    value.split_whitespace().next()?.parse().ok()
}

fn host_load_average() -> Option<f32> {
    fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn containers_notice(message: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(message.as_bytes()).collect();
    format!("/containers?notice={encoded}")
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

pub(super) async fn infrastructure(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn stacks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn stack_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
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

pub(super) async fn authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    let session = cookie_value(headers, state.config.session_cookie_name());
    state.sessions.is_valid(session).await
}

pub(super) fn page_csrf(state: &AppState, headers: &HeaderMap) -> (String, Vec<String>) {
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

pub(super) fn post_is_allowed(state: &AppState, headers: &HeaderMap, token: &str) -> bool {
    origin_is_valid(headers, &state.config) && csrf_is_valid(headers, &state.config, token)
}

pub(super) async fn load_providers(state: &AppState) -> anyhow::Result<Vec<ProviderConnection>> {
    with_catalog(state, |catalog| catalog.list_provider_connections()).await
}

pub(super) async fn with_catalog<T, F>(state: &AppState, operation: F) -> anyhow::Result<T>
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

pub(super) fn truncate(value: &str, limit: usize) -> String {
    let mut result: String = value.chars().take(limit).collect();
    if value.chars().count() > limit {
        result.push('…');
    }
    result
}

pub(super) fn internal_error(error: anyhow::Error) -> Response {
    tracing::error!(error = %error, "request failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
}

fn render_login(
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

pub(super) fn html_response(status: StatusCode, markup: Markup, cookies: Vec<String>) -> Response {
    let mut response = (status, Html(markup.into_string())).into_response();
    set_cookies(&mut response, cookies);
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

pub(super) fn redirect_response(location: &str, cookies: Vec<String>) -> Response {
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

fn origin_is_valid(headers: &HeaderMap, config: &PublicConfig) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|origin| origin == "null" || origin == config.base_url)
}

fn login_key(headers: &HeaderMap) -> String {
    headers
        .get("cf-connecting-ip")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.parse::<std::net::IpAddr>().is_ok())
        .unwrap_or("local")
        .to_owned()
}

pub(super) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = cookie_value(&headers, state.config.session_cookie_name());
    if !state.sessions.is_valid(session).await {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response
            .headers_mut()
            .insert("hx-redirect", HeaderValue::from_static("/login"));
        return response;
    }
    html_response(StatusCode::OK, templates::status_badge(), Vec::new())
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

pub(super) fn asset_url(path: &str) -> String {
    format!("/assets/{path}?v={}", *ASSET_VERSION)
}

pub(super) async fn asset(Path(path): Path<String>) -> Response {
    let path = format!("assets/{}", path.trim_start_matches('/'));
    let Some(content) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    (
        [
            (CONTENT_TYPE, mime.to_string()),
            (CACHE_CONTROL, "public, max-age=3600".into()),
        ],
        content.data.into_owned(),
    )
        .into_response()
}
