//! Settings page: GitHub, build tools (official brand icons, real
//! detection), deployment defaults, and instance info — grid layout.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use maud::{html, Markup};

use super::super::AppState;
use super::{authenticated, html_response};
use anyhow::Context as _;
use hakobi::catalog::Catalog;
use hakobi::install::{Readiness, ToolStatus};

/// GET /settings — the unified settings page.
pub(crate) async fn settings_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let readiness = match load_readiness().await {
        Ok(readiness) => readiness,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let app_installed = catalog.github_app_installed().unwrap_or(false);
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

    let markup = settings_page_markup(
        &state.config.base_url,
        &state.config.username,
        app_installed,
        connection.as_ref().map(|c| c.github_login.as_str()),
        &readiness,
        &default_branch,
        auto_deploy,
    );
    html_response(StatusCode::OK, markup, Vec::new())
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

fn settings_page_markup(
    base_url: &str,
    username: &str,
    app_installed: bool,
    github_login: Option<&str>,
    readiness: &Readiness,
    default_branch: &str,
    auto_deploy: bool,
) -> Markup {
crate::web::templates::page(
        "Settings · Hakobi",
        html! {
            div class="min-h-screen bg-[#f5f5f7] lg:pl-60" {
                (crate::web::templates::sidebar("settings", base_url, username, ""))
                (crate::web::templates::mobile_header("", username))
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
                            @if app_installed {
                                span class="rounded-full bg-emerald-50 px-3 py-1.5 text-[0.7rem] font-bold text-emerald-700" { "Connected" }
                            } @else {
                                span class="rounded-full bg-amber-50 px-3 py-1.5 text-[0.7rem] font-bold text-amber-700" { "Not connected" }
                            }
                        }
                        p class="m-0 mt-2 max-w-[36rem] text-sm leading-relaxed text-muted" {
                            "Install the Hakobi GitHub App to scan repositories, build on push, and report commit statuses. Sign in with GitHub to pick your repos in environment forms."
                        }
                        div class="mt-4 flex flex-wrap items-center gap-3" {
                            @if app_installed {
                                @if let Some(login) = github_login {
                                    span class="text-sm text-muted" { "Signed in as " (login) }
                                } @else {
                                    form method="post" action="/settings/github/connect" class="m-0" {
                                        button class="rounded-lg border border-line bg-surface px-4 py-2.5 text-sm font-semibold text-ink" type="submit" { "Sign in with GitHub" }
                                    }
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
                            "Detected on the control-plane host. An environment can use a tool once its badge is green."
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
                        p class="m-0 mt-1 text-sm text-muted" { "Applied to newly created environments." }
                        form method="post" action="/settings/defaults" class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-[1fr_auto] sm:items-end" {
                            label class="grid gap-1.5" {
                                span class="text-[0.7rem] font-bold uppercase tracking-[0.08em] text-[#86868b]" { "Default branch" }
                                input name="default_branch" value=(default_branch) class="rounded-lg border border-line bg-surface px-3 py-2.5 text-sm" required;
                            }
                            div class="flex items-end gap-3" {
                                label class="flex items-center gap-2 pb-2.5 text-sm" {
                                    input name="auto_deploy_default" type="checkbox" value="1" checked[auto_deploy] class="size-4";
                                    "Auto-deploy on push"
                                }
                                button class="rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white" type="submit" { "Save" }
                            }
                        }
                    }

                    // ---------- Instance ----------
                    section class="mt-6 rounded-[0.85rem] border border-line bg-surface p-6" aria-labelledby="instance-heading" {
                        h2 id="instance-heading" class="m-0 text-[1.05rem] font-bold" { "Instance" }
                        div class="mt-4 grid grid-cols-1 gap-x-6 gap-y-3 text-sm sm:grid-cols-[10rem_1fr]" {
                            span class="font-semibold" { "Base URL" }
                            span class="break-all text-muted" { (base_url) }
                            span class="font-semibold" { "Session lifetime" }
                            span class="text-muted" { "12 hours (JWT)" }
                            span class="font-semibold" { "Signed in as" }
                            span class="text-muted" { (username) }
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
