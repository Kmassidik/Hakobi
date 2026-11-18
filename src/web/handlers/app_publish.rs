//! Explicit post-build publication through Cloudflare Tunnel and the L7 edge.

use std::time::Duration;

use axum::{
    extract::{Form, Path, Query, State},
    http::{header::CACHE_CONTROL, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

const PUBLIC_HEALTH_ATTEMPTS: usize = 10;
const PUBLIC_HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const PUBLIC_HEALTH_RETRY_DELAY: Duration = Duration::from_secs(1);

use super::{
    app_deploy::load_app, internal_error, post_is_allowed, redirect_with_notice, with_catalog,
    AppState,
};

pub(crate) async fn app_status_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let app_id = app.id.clone();
    let latest = match with_catalog(&state, move |catalog| {
        catalog.get_latest_deployment(&app_id)
    })
    .await
    {
        Ok(latest) => latest,
        Err(error) => return internal_error(error),
    };
    super::html_response(
        StatusCode::OK,
        super::templates::app_status_body(&app, latest.as_ref()),
        Vec::new(),
    )
}

pub(crate) async fn app_publication_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let (csrf_token, _) = super::page_csrf(&state, &headers);
    let domain = apps_domain(&state).await;
    super::html_response(
        StatusCode::OK,
        super::templates::app_publication_notice(
            &app,
            &csrf_token,
            domain.as_deref(),
            None,
            None,
            false,
            false,
        ),
        Vec::new(),
    )
}

pub(super) async fn apps_domain(state: &AppState) -> Option<String> {
    with_catalog(state, |catalog| {
        catalog.settings_get(crate::cloudflare::apps_route::KEY_DOMAIN)
    })
    .await
    .ok()
    .flatten()
    .filter(|domain| !domain.is_empty())
}

#[derive(Deserialize)]
pub(crate) struct AvailabilityQuery {
    #[serde(default)]
    value: String,
}

pub(crate) async fn app_subdomain_availability(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<AvailabilityQuery>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let Some(host_port) = app.host_port.filter(|_| app.display_status() == "running") else {
        return availability_response(
            "unavailable",
            "The application must be running before a subdomain can be published.",
        );
    };
    let value = query.value.trim().to_lowercase();
    if let Err(error) = hakobi::catalog::validate_app_subdomain(&value) {
        return availability_response("invalid", &error.to_string());
    }
    checked_availability(&state, &id, host_port, &value).await
}

fn availability_response(status: &str, message: &str) -> Response {
    let mut response = Json(serde_json::json!({
        "status": status,
        "message": message,
    }))
    .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn checked_availability(state: &AppState, id: &str, host_port: u16, value: &str) -> Response {
    match catalog_subdomain_owner(state, id, value).await {
        Ok(Some(_)) => {
            return availability_response(
                "unavailable",
                "Another application already uses this subdomain.",
            );
        }
        Ok(None) => {}
        Err(_) => {
            return availability_response(
                "unavailable",
                "Availability could not be verified. Try again.",
            );
        }
    }
    let settings = match publication_settings(state).await {
        Ok(settings) => settings,
        Err(_) => {
            return availability_response(
                "unavailable",
                "Cloudflare publication settings are unavailable.",
            );
        }
    };
    route_availability_response(&settings, value, host_port).await
}

async fn route_availability_response(
    settings: &crate::cloudflare::apps_route::CfSettings,
    value: &str,
    host_port: u16,
) -> Response {
    let hostname = format!("{value}.{}", settings.domain);
    use crate::cloudflare::apps_route::AppRouteAvailability;
    match crate::cloudflare::apps_route::app_route_availability_with(settings, value, host_port)
        .await
    {
        Ok(AppRouteAvailability::Available) => {
            availability_response("available", &format!("{hostname} is available."))
        }
        Ok(AppRouteAvailability::Adoptable) => availability_response(
            "adoptable",
            &format!(
                "The existing Hakobi route for {hostname} matches this app and can be attached."
            ),
        ),
        Err(error) if error.to_string().contains("subdomain is already in use") => {
            availability_response(
                "unavailable",
                "This subdomain is already in use or points to a different target.",
            )
        }
        Err(_) => availability_response(
            "unavailable",
            "Availability could not be verified. Try again.",
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct PublishForm {
    csrf_token: String,
    subdomain: String,
}

pub(crate) async fn app_publish(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<PublishForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let subdomain = form.subdomain.trim().to_lowercase();
    let attempted = form.subdomain.as_str();
    let Some(host_port) = app.host_port.filter(|_| app.display_status() == "running") else {
        return publication_result(
            &state,
            &headers,
            &id,
            Some(attempted),
            "The application must be running before it can be published.",
            true,
        )
        .await;
    };
    if let Err(error) = hakobi::catalog::validate_app_subdomain(&subdomain) {
        return publication_result(
            &state,
            &headers,
            &id,
            Some(attempted),
            &error.to_string(),
            true,
        )
        .await;
    }
    finish_publication(&state, &headers, &id, &subdomain, attempted, host_port).await
}

async fn finish_publication(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    subdomain: &str,
    attempted: &str,
    host_port: u16,
) -> Response {
    match publish_route(state, id, subdomain, host_port).await {
        Ok(hostname) => {
            if let Err(error) = set_subdomain(state, id, Some(subdomain)).await {
                let notice = format!(
                    "The route is ready, but saving publication failed: {error:#}. Retry to recover."
                );
                return publication_result(state, headers, id, Some(attempted), &notice, true)
                    .await;
            }
            let health = public_health(&hostname).await;
            publication_health_result(state, headers, id, &hostname, health).await
        }
        Err(error) => {
            publication_result(
                state,
                headers,
                id,
                Some(attempted),
                &format!("Route publication failed: {error:#}"),
                true,
            )
            .await
        }
    }
}

async fn publication_health_result(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    hostname: &str,
    health: Result<(), String>,
) -> Response {
    match health {
        Ok(()) => {
            let notice = format!("Application is alive at https://{hostname}.");
            publication_result(state, headers, id, None, &notice, false).await
        }
        Err(error) => {
            let mut notice = format!(
                "Public health verification failed for https://{hostname}: {error}. \
                 The managed route remains configured; retry publication."
            );
            if let Err(clear_error) = set_subdomain(state, id, None).await {
                notice.push_str(&format!(
                    " Clearing the saved assignment also failed: {clear_error:#}."
                ));
            }
            publication_result(state, headers, id, Some(hostname), &notice, true).await
        }
    }
}

async fn publication_result(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    attempted: Option<&str>,
    notice: &str,
    is_error: bool,
) -> Response {
    let boosted = headers.contains_key("hx-request");
    if !boosted && !is_error {
        return redirect_with_notice(&format!("/apps/{id}"), notice);
    }
    let mut app = match load_app(state, id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    if is_error {
        app.subdomain = None;
    }
    let (csrf_token, cookies) = super::page_csrf(state, headers);
    let domain = apps_domain(state).await;
    let status = if is_error {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    super::html_response(
        status,
        super::templates::app_publication_notice(
            &app,
            &csrf_token,
            domain.as_deref(),
            Some(notice),
            attempted,
            is_error,
            !boosted,
        ),
        if boosted { Vec::new() } else { cookies },
    )
}

async fn set_subdomain(state: &AppState, id: &str, subdomain: Option<&str>) -> anyhow::Result<()> {
    let app_id = id.to_owned();
    let saved = subdomain.map(str::to_owned);
    with_catalog(state, move |catalog| {
        catalog.set_app_subdomain(&app_id, saved.as_deref())
    })
    .await
}

async fn publication_settings(
    state: &AppState,
) -> anyhow::Result<crate::cloudflare::apps_route::CfSettings> {
    with_catalog(state, |catalog| {
        crate::cloudflare::apps_route::load_settings(catalog)
    })
    .await
}

async fn publish_route(
    state: &AppState,
    id: &str,
    subdomain: &str,
    host_port: u16,
) -> anyhow::Result<String> {
    ensure_catalog_available(state, id, subdomain).await?;
    let settings = publication_settings(state).await?;
    crate::cloudflare::apps_route::publish_app_route_with(&settings, subdomain, host_port).await
}

async fn ensure_catalog_available(
    state: &AppState,
    id: &str,
    subdomain: &str,
) -> anyhow::Result<()> {
    let owner = catalog_subdomain_owner(state, id, subdomain).await?;
    anyhow::ensure!(
        owner.is_none(),
        "subdomain is already assigned to another application"
    );
    Ok(())
}

async fn catalog_subdomain_owner(
    state: &AppState,
    id: &str,
    subdomain: &str,
) -> anyhow::Result<Option<String>> {
    let app_id = id.to_owned();
    let wanted = subdomain.to_owned();
    with_catalog(state, move |catalog| {
        let owner = catalog.list_apps()?.into_iter().find(|app| {
            app.id != app_id
                && app
                    .subdomain
                    .as_deref()
                    .is_some_and(|current| current.eq_ignore_ascii_case(&wanted))
        });
        Ok(owner.map(|app| app.name))
    })
    .await
}

async fn public_health(hostname: &str) -> Result<(), String> {
    verify_public_health(&format!("https://{hostname}/"), PUBLIC_HEALTH_RETRY_DELAY).await
}

async fn verify_public_health(url: &str, retry_delay: Duration) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(PUBLIC_HEALTH_TIMEOUT)
        .build()
        .map_err(|error| format!("could not create health-check client: {error}"))?;
    let mut last_error = None;
    for attempt in 0..PUBLIC_HEALTH_ATTEMPTS {
        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => last_error = Some(format!("HTTP {}", response.status())),
            Err(error) => last_error = Some(format!("network error: {error}")),
        }
        if attempt + 1 < PUBLIC_HEALTH_ATTEMPTS {
            tokio::time::sleep(retry_delay).await;
        }
    }
    Err(last_error.expect("health verification always makes an attempt"))
}

#[cfg(test)]
mod tests;
