//! Settings page: GitHub, build tools (official brand icons, real
//! detection), deployment defaults, and instance info — grid layout.

use serde::Deserialize;

mod cloudflare;
mod domains;
mod page;
mod readiness;

pub(crate) use cloudflare::cloudflare_save;
pub(crate) use domains::{domain_create, domain_delete, domain_update};
pub(crate) use page::settings_page;

#[derive(serde::Serialize)]
pub(crate) struct CfDomainRow {
    pub name: String,
    pub record_type: String,
    pub proxied: bool,
    pub service: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CloudflareForm {
    pub(crate) csrf_token: String,
    pub(crate) account_id: String,
    pub(crate) zone_id: String,
    pub(crate) tunnel_id: String,
    pub(crate) domain: String,
    pub(crate) api_token: String,
}

#[derive(Deserialize)]
pub(crate) struct DomainForm {
    pub(crate) csrf_token: String,
    pub(crate) hostname: String,
    pub(crate) port: u16,
    #[serde(default)]
    pub(crate) record_id: String,
}
