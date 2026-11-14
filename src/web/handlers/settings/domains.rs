use axum::{
    extract::{Form, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::catalog::Catalog;

use super::super::{authenticated, internal_error, post_is_allowed, redirect_with_notice};
use super::DomainForm;
use crate::{
    cloudflare::{apps_route::CfSettings, CloudflareClient},
    web::AppState,
};

async fn cf_client_settings(state: &AppState) -> anyhow::Result<(CfSettings, Catalog)> {
    let catalog = Catalog::open((*state.catalog_path).clone())?;
    let settings = crate::cloudflare::apps_route::load_settings(&catalog)?;
    Ok((settings, catalog))
}

async fn domain_client(
    state: &AppState,
    headers: &HeaderMap,
    csrf_token: &str,
) -> Result<(CfSettings, CloudflareClient), Box<Response>> {
    if !authenticated(state, headers).await {
        return Err(Box::new(
            axum::response::Redirect::to("/login").into_response(),
        ));
    }
    if !post_is_allowed(state, headers, csrf_token) {
        return Err(Box::new(StatusCode::FORBIDDEN.into_response()));
    }
    let (settings, _catalog) = cf_client_settings(state).await.map_err(internal_error)?;
    let client = CloudflareClient::new(settings.api_token.clone()).map_err(internal_error)?;
    Ok((settings, client))
}

fn validate_hostname(hostname: &str, domain: &str) -> Result<String, String> {
    let name = hostname.trim().to_lowercase();
    if name.is_empty() {
        return Err("Hostname is required.".into());
    }
    let full = format!("{name}.{domain}");
    if !full
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
    {
        return Err("Hostname must be ASCII letters, digits, dots, or dashes.".into());
    }
    Ok(full)
}

/// POST /settings/cloudflare/domains — create a subdomain route.
pub(crate) async fn domain_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<DomainForm>,
) -> Response {
    let (settings, client) = match domain_client(&state, &headers, &form.csrf_token).await {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    let Ok(full) = validate_hostname(&form.hostname, &settings.domain) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let tunnel_id = match settings.tunnel_id.parse() {
        Ok(id) => id,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match client
        .create_subdomain(&settings.zone_id, &full, tunnel_id, form.port)
        .await
    {
        Ok(_) => client
            .upsert_ingress_route(&settings.account_id, tunnel_id, &full, form.port)
            .await
            .map(|_| redirect_with_notice("/settings", &format!("Subdomain {full} created.")))
            .unwrap_or_else(internal_error),
        Err(error) => domain_create_error(error),
    }
}

fn domain_create_error(error: anyhow::Error) -> Response {
    let status =
        if error.to_string().contains("81053") || error.to_string().contains("already exists") {
            StatusCode::CONFLICT
        } else {
            StatusCode::BAD_GATEWAY
        };
    (
        status,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

/// POST /settings/cloudflare/domains/update — repoint an existing record.
pub(crate) async fn domain_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<DomainForm>,
) -> Response {
    let (settings, client) = match domain_client(&state, &headers, &form.csrf_token).await {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    let Ok(full) = validate_hostname(&form.hostname, &settings.domain) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let tunnel_id = match settings.tunnel_id.parse() {
        Ok(id) => id,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match client
        .update_subdomain(
            &settings.zone_id,
            &form.record_id,
            &full,
            tunnel_id,
            form.port,
        )
        .await
    {
        Ok(()) => client
            .upsert_ingress_route(&settings.account_id, tunnel_id, &full, form.port)
            .await
            .map(|_| redirect_with_notice("/settings", &format!("Subdomain {full} updated.")))
            .unwrap_or_else(internal_error),
        Err(error) => internal_error(error),
    }
}

/// POST /settings/cloudflare/domains/delete — remove record + ingress rule.
pub(crate) async fn domain_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<DomainForm>,
) -> Response {
    let (settings, client) = match domain_client(&state, &headers, &form.csrf_token).await {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    let tunnel_id = match settings.tunnel_id.parse() {
        Ok(id) => id,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let records = match client
        .find_dns_records(&settings.zone_id, &form.hostname.trim().to_lowercase())
        .await
    {
        Ok(records) => records,
        Err(error) => return internal_error(error),
    };
    let mut notice = String::new();
    for record in records {
        match client
            .delete_hakobi_record(&settings.zone_id, &record)
            .await
        {
            Ok(true) => notice = format!("Subdomain {} deleted.", record.name),
            Ok(false) => {
                notice = format!(
                    "{} is not managed by Hakobi — delete it from the Cloudflare dashboard.",
                    record.name
                );
            }
            Err(error) => return internal_error(error),
        }
    }
    if let Err(error) = client
        .remove_ingress_route(
            &settings.account_id,
            tunnel_id,
            &form.hostname.trim().to_lowercase(),
        )
        .await
    {
        return internal_error(error);
    }
    redirect_with_notice("/settings", &notice)
}
