use axum::{
    extract::{Form, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::catalog::Catalog;

use super::super::{
    authenticated, internal_error, post_is_allowed, redirect_with_error, redirect_with_notice,
    with_catalog,
};
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

async fn delete_dns_records(
    client: &CloudflareClient,
    zone_id: &str,
    records: Vec<crate::cloudflare::DnsRecord>,
) -> anyhow::Result<(bool, String)> {
    let mut managed_any = false;
    let mut notice = "No Hakobi-managed DNS record found.".to_owned();
    for record in records {
        if client.delete_hakobi_record(zone_id, &record).await? {
            managed_any = true;
            notice = format!("Subdomain {} deleted.", record.name);
        } else {
            notice = format!(
                "{} is not managed by Hakobi — delete it from the Cloudflare dashboard.",
                record.name
            );
        }
    }
    Ok((managed_any, notice))
}

#[derive(serde::Deserialize)]
pub(crate) struct DomainDeleteForm {
    csrf_token: String,
    hostname: String,
    confirm_name: String,
}

async fn assigned_app(
    state: &AppState,
    domain: &str,
    hostname: &str,
) -> anyhow::Result<Option<String>> {
    let domain = domain.to_owned();
    let hostname = hostname.to_owned();
    with_catalog(state, move |catalog| {
        let owner = catalog.list_apps()?.into_iter().find(|app| {
            app.subdomain.as_ref().is_some_and(|subdomain| {
                format!("{subdomain}.{domain}").eq_ignore_ascii_case(&hostname)
            })
        });
        Ok(owner.map(|app| app.name))
    })
    .await
}

async fn deletion_error(
    state: &AppState,
    domain: &str,
    form: &DomainDeleteForm,
) -> anyhow::Result<Option<Response>> {
    let hostname = form.hostname.trim().to_lowercase();
    if form.confirm_name != hostname {
        return Ok(Some(redirect_with_error(
            "/settings",
            "Type the exact hostname to confirm deletion; no DNS or ingress changes were made.",
        )));
    }
    match assigned_app(state, domain, &hostname).await? {
        Some(app) => Ok(Some(redirect_with_error(
            "/settings",
            &format!("{hostname} is assigned to app {app}; unpublish that app first."),
        ))),
        None => Ok(None),
    }
}

/// POST /settings/cloudflare/domains/delete — remove record + ingress rule.
pub(crate) async fn domain_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<DomainDeleteForm>,
) -> Response {
    let (settings, client) = match domain_client(&state, &headers, &form.csrf_token).await {
        Ok(pair) => pair,
        Err(response) => return *response,
    };
    let hostname = form.hostname.trim().to_lowercase();
    match deletion_error(&state, &settings.domain, &form).await {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(error) => return internal_error(error),
    }
    let tunnel_id = match settings.tunnel_id.parse() {
        Ok(id) => id,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let records = match client.find_dns_records(&settings.zone_id, &hostname).await {
        Ok(records) => records,
        Err(error) => return internal_error(error),
    };
    let (managed_any, notice) = match delete_dns_records(&client, &settings.zone_id, records).await
    {
        Ok(result) => result,
        Err(error) => return internal_error(error),
    };
    if !managed_any {
        return redirect_with_error("/settings", &notice);
    }
    if let Err(error) = client
        .remove_ingress_route(&settings.account_id, tunnel_id, &hostname)
        .await
    {
        return internal_error(error);
    }
    redirect_with_notice("/settings", &notice)
}
