use axum::{
    extract::{Form, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hakobi::catalog::Catalog;
use rusqlite::{params, Connection, Transaction};

use super::super::{
    authenticated, internal_error, post_is_allowed, redirect_with_error, redirect_with_notice,
    with_catalog,
};
use super::{CfDomainRow, CloudflareForm};
use crate::web::AppState;

/// Gathers the zone's CNAME/A records merged with tunnel ingress targets.
pub(super) async fn fetch_cf_domains(state: &AppState) -> anyhow::Result<Vec<CfDomainRow>> {
    use crate::cloudflare::apps_route::load_settings;

    let catalog = Catalog::open((*state.catalog_path).clone())?;
    let settings = load_settings(&catalog)?;
    let client = crate::cloudflare::CloudflareClient::new(settings.api_token.clone())?;
    let tunnel_id = settings.tunnel_id.parse()?;

    let mut rows: Vec<CfDomainRow> = client
        .list_dns_records(&settings.zone_id)
        .await?
        .into_iter()
        .filter(|record| record.r#type == "CNAME" || record.r#type == "A")
        .map(|record| CfDomainRow {
            name: record.name,
            record_type: record.r#type,
            proxied: record.proxied,
            service: None,
            managed: record
                .comment
                .as_deref()
                .is_some_and(|comment| comment.starts_with("Managed by Hakobi")),
        })
        .collect();

    let ingress = client
        .get_tunnel_ingress(&settings.account_id, tunnel_id)
        .await?;
    for rule in ingress {
        let Some(hostname) = rule.hostname else {
            continue;
        };
        if let Some(row) = rows.iter_mut().find(|row| row.name == hostname) {
            row.service = Some(rule.service);
        } else {
            rows.push(CfDomainRow {
                name: hostname,
                record_type: "tunnel".to_owned(),
                proxied: true,
                service: Some(rule.service),
                managed: false,
            });
        }
    }
    Ok(rows)
}

/// POST /settings/cloudflare — saves tunnel + DNS credentials.
pub(crate) async fn cloudflare_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CloudflareForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(message) = cloudflare_validation_error(&form) {
        return redirect_with_error("/settings", &message);
    }
    let path = (*state.catalog_path).clone();
    let result = with_catalog(&state, move |catalog| {
        persist_cloudflare_settings(catalog, &path, &form)
    })
    .await;
    match result {
        Ok(true) => redirect_with_notice("/settings", "Cloudflare settings saved."),
        Ok(false) => redirect_with_error(
            "/settings",
            "Cloudflare API token is required for initial configuration.",
        ),
        Err(error) => internal_error(error),
    }
}

fn cloudflare_validation_error(form: &CloudflareForm) -> Option<String> {
    let fields = [
        ("account ID", &form.account_id),
        ("zone ID", &form.zone_id),
        ("tunnel ID", &form.tunnel_id),
        ("application domain", &form.domain),
    ];
    fields
        .into_iter()
        .find(|(_, value)| value.trim().is_empty())
        .map(|(name, _)| format!("Cloudflare {name} is required."))
}

fn persist_cloudflare_settings(
    catalog: &Catalog,
    path: &std::path::Path,
    form: &CloudflareForm,
) -> anyhow::Result<bool> {
    use crate::cloudflare::apps_route as keys;

    let token = form.api_token.trim();
    if token.is_empty()
        && !catalog
            .settings_get(keys::KEY_API_TOKEN)?
            .is_some_and(|value| !value.is_empty())
    {
        return Ok(false);
    }
    let encrypted = if token.is_empty() {
        None
    } else {
        Some(catalog.encrypt_secret(&form.api_token)?)
    };
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    let transaction = connection.transaction()?;
    for (key, value) in [
        (keys::KEY_ACCOUNT_ID, &form.account_id),
        (keys::KEY_ZONE_ID, &form.zone_id),
        (keys::KEY_TUNNEL_ID, &form.tunnel_id),
        (keys::KEY_DOMAIN, &form.domain),
    ] {
        write_setting(&transaction, key, value)?;
    }
    if let Some(value) = encrypted {
        write_setting(&transaction, keys::KEY_API_TOKEN, &value)?;
    }
    transaction.commit()?;
    Ok(true)
}

fn write_setting(transaction: &Transaction<'_>, key: &str, value: &str) -> anyhow::Result<()> {
    transaction.execute(
        "INSERT INTO settings_kv (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}
