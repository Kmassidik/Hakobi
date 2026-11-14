use axum::{
    extract::{Form, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hakobi::catalog::Catalog;

use super::super::{
    authenticated, internal_error, post_is_allowed, redirect_with_notice, with_catalog,
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
    let result = with_catalog(&state, move |catalog| {
        catalog.settings_set(
            crate::cloudflare::apps_route::KEY_ACCOUNT_ID,
            &form.account_id,
        )?;
        catalog.settings_set(crate::cloudflare::apps_route::KEY_ZONE_ID, &form.zone_id)?;
        catalog.settings_set(
            crate::cloudflare::apps_route::KEY_TUNNEL_ID,
            &form.tunnel_id,
        )?;
        catalog.settings_set(crate::cloudflare::apps_route::KEY_DOMAIN, &form.domain)?;
        catalog.settings_set_encrypted(
            crate::cloudflare::apps_route::KEY_API_TOKEN,
            &form.api_token,
        )?;
        Ok(())
    })
    .await;
    match result {
        Ok(()) => redirect_with_notice("/settings", "Cloudflare settings saved."),
        Err(error) => internal_error(error),
    }
}
