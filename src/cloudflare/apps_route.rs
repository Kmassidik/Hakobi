//! Cloudflare Tunnel routing for deployed apps.
//!
//! After a successful deploy the executor calls [`sync_app_route`] to point
//! `{subdomain}` at the host port the container was published on; app deletion
//! calls [`remove_app_route`]. Credentials live in `settings_kv`
//! (`cf_account_id`, `cf_zone_id`, `cf_tunnel_id`, and the API token encrypted
//! via the catalog vault under `cf_api_token`).

use crate::cloudflare::CloudflareClient;
use anyhow::Context;
use hakobi::catalog::Catalog;

/// Setting keys for the Cloudflare integration.
pub const KEY_ACCOUNT_ID: &str = "cf_account_id";
pub const KEY_ZONE_ID: &str = "cf_zone_id";
pub const KEY_TUNNEL_ID: &str = "cf_tunnel_id";
pub const KEY_API_TOKEN: &str = "cf_api_token";
/// Zone root that app subdomains attach to (e.g. `glicc.id`).
pub const KEY_DOMAIN: &str = "cf_apps_domain";

/// Decrypted Cloudflare settings for route management.
#[derive(Clone)]
pub struct CfSettings {
    pub account_id: String,
    pub zone_id: String,
    pub tunnel_id: String,
    pub domain: String,
    pub api_token: String,
}

/// Loads and decrypts all settings. Errors when any are missing.
pub fn load_settings(catalog: &Catalog) -> anyhow::Result<CfSettings> {
    let need = |key: &str| -> anyhow::Result<String> {
        catalog
            .settings_get(key)?
            .filter(|value| !value.is_empty())
            .context(key.to_owned())
    };
    Ok(CfSettings {
        account_id: need(KEY_ACCOUNT_ID)?,
        zone_id: need(KEY_ZONE_ID)?,
        tunnel_id: need(KEY_TUNNEL_ID)?,
        domain: need(KEY_DOMAIN)?,
        api_token: catalog.decrypt_secret(&need(KEY_API_TOKEN)?)?,
    })
}

fn subdomain_for(domain: &str, app_name: &str) -> String {
    format!("{app_name}.{domain}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRouteAvailability {
    Available,
    Adoptable,
}

struct RouteInspection {
    hostname: String,
    dns_exists: bool,
    adoptable: bool,
}

async fn inspect_app_route(
    client: &CloudflareClient,
    settings: &CfSettings,
    tunnel_id: uuid::Uuid,
    app_name: &str,
    host_port: u16,
) -> anyhow::Result<RouteInspection> {
    let hostname = subdomain_for(&settings.domain, app_name);
    let records = client
        .find_dns_records(&settings.zone_id, &hostname)
        .await?;
    let target = format!("{tunnel_id}.cfargotunnel.com");
    anyhow::ensure!(
        records.iter().all(|record| {
            crate::cloudflare::is_hakobi_record(record)
                && record.r#type.eq_ignore_ascii_case("CNAME")
                && record
                    .content
                    .trim_end_matches('.')
                    .eq_ignore_ascii_case(&target)
                && record.proxied
        }),
        "subdomain is already in use by an unmanaged or wrong-target DNS record"
    );
    let rules = client
        .get_tunnel_ingress(&settings.account_id, tunnel_id)
        .await?;
    let service = format!("http://127.0.0.1:{host_port}");
    let mut route_exists = false;
    for rule in rules
        .iter()
        .filter(|rule| rule.hostname.as_deref() == Some(hostname.as_str()))
    {
        route_exists = true;
        anyhow::ensure!(
            rule.service == service,
            "subdomain is already in use by a route with a different target"
        );
    }
    Ok(RouteInspection {
        hostname,
        dns_exists: !records.is_empty(),
        adoptable: !records.is_empty() || route_exists,
    })
}

pub async fn app_route_availability_with(
    settings: &CfSettings,
    app_name: &str,
    host_port: u16,
) -> anyhow::Result<AppRouteAvailability> {
    let client = CloudflareClient::new(settings.api_token.clone())?;
    let tunnel_id = settings
        .tunnel_id
        .parse()
        .context("cf_tunnel_id is not a valid UUID")?;
    let inspected = inspect_app_route(&client, settings, tunnel_id, app_name, host_port).await?;
    Ok(if inspected.adoptable {
        AppRouteAvailability::Adoptable
    } else {
        AppRouteAvailability::Available
    })
}

/// Publishes a previously unassigned hostname, adopting only an interrupted
/// Hakobi publication whose DNS and ingress targets match this app.
pub async fn publish_app_route_with(
    settings: &CfSettings,
    app_name: &str,
    host_port: u16,
) -> anyhow::Result<String> {
    let client = CloudflareClient::new(settings.api_token.clone())?;
    let tunnel_id = settings
        .tunnel_id
        .parse()
        .context("cf_tunnel_id is not a valid UUID")?;
    let inspected = inspect_app_route(&client, settings, tunnel_id, app_name, host_port).await?;
    client
        .upsert_ingress_route(
            &settings.account_id,
            tunnel_id,
            &inspected.hostname,
            host_port,
        )
        .await?;
    if !inspected.dns_exists {
        client
            .create_dns_record(&settings.zone_id, &inspected.hostname, tunnel_id)
            .await?;
    }
    Ok(inspected.hostname)
}

/// Same as [`sync_app_route`] with pre-loaded settings (non-blocking caller).
pub async fn sync_app_route_with(
    settings: &CfSettings,
    app_name: &str,
    host_port: u16,
) -> anyhow::Result<String> {
    let account_id = settings.account_id.clone();
    let zone_id = settings.zone_id.clone();
    let token = settings.api_token.clone();
    let client = CloudflareClient::new(token)?;
    let tunnel_id = settings
        .tunnel_id
        .parse()
        .context("cf_tunnel_id is not a valid UUID")?;
    let hostname = subdomain_for(&settings.domain, app_name);

    client
        .upsert_ingress_route(&account_id, tunnel_id, &hostname, host_port)
        .await?;

    // DNS CNAME: hostname -> <tunnel-id>.cfargotunnel.com (create if missing).
    if client
        .find_dns_records(&zone_id, &hostname)
        .await?
        .is_empty()
    {
        client
            .create_dns_record(&zone_id, &hostname, tunnel_id)
            .await?;
    }

    Ok(hostname)
}

/// Removes the app's ingress rule and DNS record.
pub async fn remove_app_route_with(settings: &CfSettings, app_name: &str) -> anyhow::Result<()> {
    let client = CloudflareClient::new(settings.api_token.clone())?;
    let tunnel_id = settings
        .tunnel_id
        .parse()
        .context("cf_tunnel_id is not a valid UUID")?;
    let hostname = subdomain_for(&settings.domain, app_name);
    client
        .remove_ingress_route(&settings.account_id, tunnel_id, &hostname)
        .await?;
    for record in client
        .find_dns_records(&settings.zone_id, &hostname)
        .await?
    {
        client
            .delete_hakobi_record(&settings.zone_id, &record)
            .await?;
    }
    Ok(())
}
