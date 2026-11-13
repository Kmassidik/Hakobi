//! Settings page: GitHub, build tools (official brand icons, real
//! detection), deployment defaults, and instance info — grid layout.

use axum::{
    extract::Form,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use maud::{html, Markup};

use super::super::AppState;
use serde::Deserialize;

use super::{
    authenticated, html_response, internal_error, page_csrf, post_is_allowed, redirect_with_notice,
    with_catalog,
};
use anyhow::Context as _;
use hakobi::catalog::Catalog;
use hakobi::install::{Readiness, ToolStatus};

/// GET /settings — the unified settings page.
pub(crate) async fn settings_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let readiness = match load_readiness().await {
        Ok(readiness) => readiness,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let app_installed = catalog.github_app_installed().unwrap_or(false);
    let app_registered = catalog.github_app_registered().unwrap_or(false);
    let app_slug = catalog.github_app_slug().unwrap_or(None);
    let connection = catalog
        .load_github_connection(&state.config.username)
        .ok()
        .flatten();
    let default_branch = catalog
        .settings_get("default_branch")
        .ok()
        .flatten()
        .unwrap_or_else(|| "main".to_owned());
    let auto_deploy = catalog
        .settings_get("auto_deploy_default")
        .ok()
        .flatten()
        .map(|v| v == "1")
        .unwrap_or(true);

    // List CF domains (records + ingress) when configured; failures degrade to
    // an empty list so the page still renders.
    let cf_domains = fetch_cf_domains(&state).await.unwrap_or_default();
    let page = SettingsPage {
        base_url: &state.config.base_url,
        username: &state.config.username,
        app_installed,
        app_registered,
        app_slug: app_slug.as_deref(),
        github_login: connection.as_ref().map(|c| c.github_login.as_str()),
        default_branch: &default_branch,
        auto_deploy,
        csrf_token: &csrf_token,
        cf_domains,
    };
    let markup = settings_page_markup(&page, &readiness);
    html_response(StatusCode::OK, markup, cookies)
}

/// Loads tool readiness from the host agent (falls back to local detect).
async fn load_readiness() -> anyhow::Result<Readiness> {
    let agent = super::super::agent_client::HostAgent::from_env();
    if let Some(agent) = agent {
        if let Ok(readiness) = agent.readiness().await {
            return Ok(readiness);
        }
    }
    tokio::task::spawn_blocking(hakobi::install::detect)
        .await
        .context("host readiness probe")
}

struct SettingsPage<'a> {
    base_url: &'a str,
    username: &'a str,
    app_installed: bool,
    app_registered: bool,
    app_slug: Option<&'a str>,
    github_login: Option<&'a str>,
    default_branch: &'a str,
    auto_deploy: bool,
    csrf_token: &'a str,
    cf_domains: Vec<CfDomainRow>,
}

#[derive(serde::Serialize)]
pub(crate) struct CfDomainRow {
    pub name: String,
    pub record_type: String,
    pub proxied: bool,
    pub service: Option<String>,
}

fn settings_page_markup(page: &SettingsPage<'_>, readiness: &Readiness) -> Markup {
    crate::web::templates::page(
        "Settings · Hakobi",
        html! {
            div class="min-h-screen bg-[#f5f5f7] lg:pl-60" {
                (crate::web::templates::sidebar("settings", page.base_url, page.username, page.csrf_token))
                (crate::web::templates::mobile_header(page.csrf_token, page.username))
                main class="mx-auto w-[92%] max-w-[60rem] pb-24 pt-10 lg:pt-16" {
                    p class="mb-3 text-[0.65rem] font-semibold uppercase tracking-[0.14em] text-brand-bright" { "Settings" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "Settings" }

                    // ---------- Integrations: GitHub ----------
                    section class="mt-8 rounded-[0.85rem] border border-line bg-surface p-6" aria-labelledby="gh-heading" {
                        div class="flex items-center justify-between gap-4" {
                            div class="flex items-center gap-3" {
                                i class="fa-brands fa-github text-[1.5rem] text-ink" aria-hidden="true" {}
                                h2 id="gh-heading" class="m-0 text-[1.05rem] font-bold" { "GitHub" }
                            }
                            @if page.app_installed {
                                span class="rounded-full bg-emerald-50 px-3 py-1.5 text-[0.7rem] font-bold text-emerald-700" { "Connected" }
                            } @else if page.app_registered {
                                span class="rounded-full bg-amber-50 px-3 py-1.5 text-[0.7rem] font-bold text-amber-700" { "Registered — install pending" }
                            } @else {
                                span class="rounded-full bg-amber-50 px-3 py-1.5 text-[0.7rem] font-bold text-amber-700" { "Not connected" }
                            }
                        }
                        p class="m-0 mt-2 max-w-[36rem] text-sm leading-relaxed text-muted" {
                            "Install the Hakobi GitHub App to scan repositories, build on push, and report commit statuses. Sign in with GitHub to pick your repos in the app form."
                        }
                        div class="mt-4 flex flex-wrap items-center gap-3" {
                            @if page.app_installed {
                                @if let Some(login) = page.github_login {
                                    span class="text-sm text-muted" { "Signed in as " (login) }
                                } @else {
                                    form method="post" action="/settings/github/connect" class="m-0" {
                                        button class="rounded-lg border border-line bg-surface px-4 py-2.5 text-sm font-semibold text-ink" type="submit" { "Sign in with GitHub" }
                                    }
                                }
                            } @else if page.app_registered {
                                @if let Some(slug) = page.app_slug {
                                    a href=(format!("https://github.com/apps/{slug}/installations/new"))
                                        class="rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white no-underline" {
                                        "Install on GitHub"
                                    }
                                }
                                form method="post" action="/settings/github/recheck" class="m-0" {
                                    button class="rounded-lg border border-line bg-surface px-4 py-2.5 text-sm font-semibold text-ink" type="submit" { "Check installation" }
                                }
                            } @else {
                                form method="post" action="/settings/github/install" class="m-0" {
                                    button class="rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white" type="submit" { "Install GitHub App" }
                                }
                                span class="text-xs text-muted" { "Approve on github.com — no passwords shared." }
                            }
                        }
                    }

                    // ---------- Build tools grid (moved from overview) ----------
                    section class="mt-6 rounded-[0.85rem] border border-line bg-surface p-6" aria-labelledby="tools-heading" {
                        h2 id="tools-heading" class="m-0 text-[1.05rem] font-bold" { "Build tools" }
                        p class="m-0 mt-1 text-sm text-muted" {
                            "Detected on the control-plane host. An app can use a tool once its badge is green."
                        }
                        div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3" {
                            (tool_tile("fab fa-docker", "Docker", &readiness.docker, "Container builds, compose, deploys"))
                            (tool_tile("fas fa-cube", "Nixpacks", &readiness.nixpacks, "Zero-config builds — pick as build type"))
                            (tool_tile("fas fa-dharmachakra", "Kubernetes", &readiness.kubectl, "Planned — deploy target not wired yet"))
                        }
                    }

                    // ---------- Deployment defaults ----------
                    section class="mt-6 rounded-[0.85rem] border border-line bg-surface p-6" aria-labelledby="defaults-heading" {
                        h2 id="defaults-heading" class="m-0 text-[1.05rem] font-bold" { "Deployment defaults" }
                        p class="m-0 mt-1 text-sm text-muted" { "Applied to newly created apps." }
                        form method="post" action="/settings/defaults" class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-[1fr_auto] sm:items-end" {
                            label class="grid gap-1.5" {
                                span class="text-[0.7rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Default branch" }
                                input name="page.default_branch" value=(page.default_branch) class="rounded-lg border border-line bg-surface px-3 py-2.5 text-sm" required;
                            }
                            div class="flex items-end gap-3" {
                                label class="flex items-center gap-2 pb-2.5 text-sm" {
                                    input name="page.auto_deploy_default" type="checkbox" value="1" checked[page.auto_deploy] class="size-4";
                                    "Auto-deploy on push"
                                }
                                button class="rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white" type="submit" { "Save" }
                            }
                        }
                    }

                    // ---------- Cloudflare ----------
                section class="mt-6 rounded-[0.85rem] border border-line bg-surface p-6" aria-labelledby="cf-heading" {
                    h2 id="cf-heading" class="m-0 text-[1.05rem] font-bold" { "Cloudflare (app domains)" }
                    p class="m-0 mt-1 text-sm text-muted" {
                        "Each deployed app gets a public subdomain on your domain through your Cloudflare Tunnel."
                    }
                    @if !page.cf_domains.is_empty() {
                        div class="mt-4 overflow-x-auto rounded-lg border border-line" {
                            table class="m-0 w-full text-sm" {
                                thead {
                                    tr class="border-b border-line text-left text-[0.7rem] font-bold uppercase tracking-[0.08em] text-muted" {
                                        th class="px-3 py-2" { "Hostname" }
                                        th class="px-3 py-2" { "Type" }
                                        th class="px-3 py-2" { "Proxied" }
                                        th class="px-3 py-2" { "Routes to" }
                                        th class="px-3 py-2 text-right" { "" }
                                    }
                                }
                                tbody {
                                    @for row in &page.cf_domains {
                                        tr class="border-b border-line/60 last:border-b-0" {
                                            td class="px-3 py-2 font-mono text-[0.8rem] font-semibold" { (row.name) }
                                            td class="px-3 py-2 text-muted" { (row.record_type) }
                                            td class="px-3 py-2" {
                                                @if row.proxied {
                                                    span class="rounded-full bg-orange-100 px-2 py-0.5 text-[0.65rem] font-bold text-orange-700" { "proxied" }
                                                } @else {
                                                    span class="text-muted" { "dns only" }
                                                }
                                            }
                                            td class="px-3 py-2 font-mono text-[0.8rem] text-muted" { (row.service.as_deref().unwrap_or("-")) }
                                            td class="px-3 py-2 text-right" {
                                                @if let Some(port) = row.service.as_deref().and_then(|service| service.rsplit(':').next()).and_then(|port| port.parse::<u16>().ok()) {
                                                    form method="post" action="/settings/cloudflare/domains/delete" class="m-0 inline" {
                                                        input type="hidden" name="csrf_token" value=(page.csrf_token);
                                                        input type="hidden" name="hostname" value=(row.name);
                                                        input type="hidden" name="record_id" value="";
                                                        input type="hidden" name="port" value=(port);
                                                        button class="rounded-md border border-danger/40 px-2 py-1 text-[0.7rem] font-bold text-danger transition hover:bg-danger/5" type="submit" { "Delete route" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    form method="post" action="/settings/cloudflare" class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2" {
                        input type="hidden" name="csrf_token" value=(page.csrf_token);
                        label class="grid gap-1 text-sm" { "Account ID"
                            input name="account_id" class="rounded-lg border border-line bg-surface px-3 py-2 text-sm" required; }
                        label class="grid gap-1 text-sm" { "Zone ID"
                            input name="zone_id" class="rounded-lg border border-line bg-surface px-3 py-2 text-sm" required; }
                        label class="grid gap-1 text-sm" { "Tunnel ID"
                            input name="tunnel_id" class="rounded-lg border border-line bg-surface px-3 py-2 text-sm" required; }
                        label class="grid gap-1 text-sm" { "Root domain (e.g. glicc.id)"
                            input name="domain" class="rounded-lg border border-line bg-surface px-3 py-2 text-sm" required; }
                        label class="grid gap-1 text-sm sm:col-span-2" { "API token (Tunnel:Edit + DNS:Edit) — stored encrypted"
                            input name="api_token" type="password" class="rounded-lg border border-line bg-surface px-3 py-2 text-sm" required; }
                        div class="sm:col-span-2" {
                            button class="rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white" type="submit" { "Save Cloudflare settings" }
                        }
                    }
                    // -- Subdomain quick-create --
                    form method="post" action="/settings/cloudflare/domains" class="mt-4 flex flex-wrap items-end gap-3 border-t border-line pt-4" {
                        input type="hidden" name="csrf_token" value=(page.csrf_token);
                        label class="grid gap-1 text-sm" { "New subdomain"
                            input name="hostname" class="rounded-lg border border-line bg-surface px-3 py-2 text-sm" placeholder="myapp" required; }
                        label class="grid gap-1 text-sm" { "Port"
                            input name="port" type="number" min="1" max="65535" class="rounded-lg border border-line bg-surface px-3 py-2 text-sm" required; }
                        button class="rounded-lg border border-line bg-surface px-4 py-2.5 text-sm font-semibold text-ink" type="submit" { "Create subdomain" }
                    }
                }

                // ---------- Instance ----------
                    section class="mt-6 rounded-[0.85rem] border border-line bg-surface p-6" aria-labelledby="instance-heading" {
                        h2 id="instance-heading" class="m-0 text-[1.05rem] font-bold" { "Instance" }
                        div class="mt-4 grid grid-cols-1 gap-x-6 gap-y-3 text-sm sm:grid-cols-[10rem_1fr]" {
                            span class="font-semibold" { "Base URL" }
                            span class="break-all text-muted" { (page.base_url) }
                            span class="font-semibold" { "Session lifetime" }
                            span class="text-muted" { "12 hours (JWT)" }
                            span class="font-semibold" { "Signed in as" }
                            span class="text-muted" { (page.username) }
                        }
                    }
                }
            }
        },
    )
}

fn tool_tile(icon: &str, name: &str, status: &ToolStatus, description: &str) -> Markup {
    let ready = matches!(status, ToolStatus::Ready { .. });
    let (badge_class, badge_text, tile_class) = if ready {
        ("bg-emerald-50 text-emerald-700", "Ready", "")
    } else {
        ("bg-amber-50 text-amber-700", "Not installed", "opacity-70")
    };
    let version = match status {
        ToolStatus::Ready { version } => version.as_str(),
        _ => "—",
    };
    html! {
        div class=(format!("rounded-xl border border-line bg-white p-4 {tile_class}")) {
            div class="flex items-center justify-between gap-2" {
                i class=(format!("{icon} text-[1.4rem] text-ink")) aria-hidden="true" {}
                span class=(format!("rounded-full px-2.5 py-1 text-[0.62rem] font-bold {badge_class}")) { (badge_text) }
            }
            p class="m-0 mt-3 font-mono text-[0.95rem] font-semibold" { (version) }
            p class="m-0 mt-1 text-[0.8rem] font-bold" { (name) }
            p class="m-0 mt-0.5 text-xs leading-snug text-muted" { (description) }
        }
    }
}

/// Gathers the zone's CNAME/A records merged with tunnel ingress targets.
async fn fetch_cf_domains(state: &AppState) -> anyhow::Result<Vec<CfDomainRow>> {
    use crate::cloudflare::apps_route::load_settings;
    use hakobi::catalog::Catalog;

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
    if !super::authenticated(&state, &headers).await {
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

async fn cf_client_settings(
    state: &AppState,
) -> anyhow::Result<(crate::cloudflare::apps_route::CfSettings, Catalog)> {
    let catalog = Catalog::open((*state.catalog_path).clone())?;
    let settings = crate::cloudflare::apps_route::load_settings(&catalog)?;
    Ok((settings, catalog))
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
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (settings, _catalog) = match cf_client_settings(&state).await {
        Ok(pair) => pair,
        Err(error) => return internal_error(error),
    };
    let Ok(full) = validate_hostname(&form.hostname, &settings.domain) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let client = match crate::cloudflare::CloudflareClient::new(settings.api_token.clone()) {
        Ok(client) => client,
        Err(error) => return internal_error(error),
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
        Err(error) => {
            let status = if error.to_string().contains("81053")
                || error.to_string().contains("already exists")
            {
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
    }
}

/// POST /settings/cloudflare/domains/update — repoint an existing record.
pub(crate) async fn domain_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<DomainForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (settings, _catalog) = match cf_client_settings(&state).await {
        Ok(pair) => pair,
        Err(error) => return internal_error(error),
    };
    let Ok(full) = validate_hostname(&form.hostname, &settings.domain) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let client = match crate::cloudflare::CloudflareClient::new(settings.api_token.clone()) {
        Ok(client) => client,
        Err(error) => return internal_error(error),
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
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (settings, _catalog) = match cf_client_settings(&state).await {
        Ok(pair) => pair,
        Err(error) => return internal_error(error),
    };
    let client = match crate::cloudflare::CloudflareClient::new(settings.api_token.clone()) {
        Ok(client) => client,
        Err(error) => return internal_error(error),
    };
    let tunnel_id = match settings.tunnel_id.parse() {
        Ok(id) => id,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    // find the record by hostname
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
