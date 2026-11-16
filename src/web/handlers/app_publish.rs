//! Explicit post-build publication through Cloudflare Tunnel and the L7 edge.

use axum::{
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

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
        super::templates::app_publication_notice(&app, &csrf_token, domain.as_deref(), None),
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
    let Some(host_port) = app.host_port.filter(|_| app.display_status() == "running") else {
        return publication_result(
            &state,
            &headers,
            &id,
            "The application must be running before it can be published.",
        )
        .await;
    };
    let subdomain = form.subdomain.trim().to_lowercase();
    if let Err(error) = hakobi::catalog::validate_app_subdomain(&subdomain) {
        return publication_result(&state, &headers, &id, &error.to_string()).await;
    }
    finish_publication(&state, &headers, &id, &subdomain, host_port).await
}

async fn finish_publication(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    subdomain: &str,
    host_port: u16,
) -> Response {
    match publish_route(state, subdomain, host_port).await {
        Ok(hostname) => {
            if let Err(error) = save_subdomain(state, id, subdomain).await {
                return internal_error(error);
            }
            let healthy = public_health(&hostname).await;
            let notice = if healthy {
                format!("Application is alive at https://{hostname}.")
            } else {
                format!(
                    "Published https://{hostname}; public health verification is still pending."
                )
            };
            publication_result(state, headers, id, &notice).await
        }
        Err(error) => {
            publication_result(
                state,
                headers,
                id,
                &format!("Route publication failed: {error:#}"),
            )
            .await
        }
    }
}

async fn publication_result(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    notice: &str,
) -> Response {
    if !headers.contains_key("hx-request") {
        return redirect_with_notice(&format!("/apps/{id}"), notice);
    }
    let app = match load_app(state, id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let (csrf_token, _) = super::page_csrf(state, headers);
    let domain = apps_domain(state).await;
    super::html_response(
        StatusCode::OK,
        super::templates::app_publication_notice(
            &app,
            &csrf_token,
            domain.as_deref(),
            Some(notice),
        ),
        Vec::new(),
    )
}

async fn save_subdomain(state: &AppState, id: &str, subdomain: &str) -> anyhow::Result<()> {
    let app_id = id.to_owned();
    let saved = subdomain.to_owned();
    with_catalog(state, move |catalog| {
        catalog.set_app_subdomain(&app_id, Some(&saved))
    })
    .await
}

async fn publish_route(
    state: &AppState,
    subdomain: &str,
    host_port: u16,
) -> anyhow::Result<String> {
    let settings = with_catalog(state, |catalog| {
        crate::cloudflare::apps_route::load_settings(catalog)
    })
    .await?;
    let client = crate::cloudflare::CloudflareClient::new(settings.api_token.clone())?;
    let hostname = format!("{subdomain}.{}", settings.domain);
    let existing = client
        .find_dns_records(&settings.zone_id, &hostname)
        .await?;
    anyhow::ensure!(existing.is_empty(), "subdomain is already in use");
    drop(client);
    crate::cloudflare::apps_route::sync_app_route_with(&settings, subdomain, host_port).await
}

async fn public_health(hostname: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let Ok(client) = client else {
        return false;
    };
    client
        .get(format!("https://{hostname}/"))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}
