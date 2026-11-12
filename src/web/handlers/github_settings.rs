//! Settings page GitHub card and OAuth connect/callback handlers.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::super::templates;
use super::super::AppState;
use super::github::manifest_exchange_inner;
use super::{authenticated, html_response, page_csrf};
use hakobi::catalog::Catalog;
use maud::{html, Markup};
use rand::RngCore;

/// GET /settings/github — connection status page.
pub(crate) async fn github_settings_page(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let app_installed = catalog.github_app_installed().unwrap_or(false);
    let connection = catalog
        .load_github_connection(&state.config.username)
        .ok()
        .flatten();
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let markup = github_settings_page_markup(
        &state.config.base_url,
        &state.config.username,
        app_installed,
        connection.as_ref().map(|c| c.github_login.as_str()),
        &csrf_token,
    );
    html_response(StatusCode::OK, markup, cookies)
}

fn github_settings_page_markup(
    base_url: &str,
    username: &str,
    app_installed: bool,
    github_login: Option<&str>,
    csrf_token: &str,
) -> Markup {
    templates::page(
        "GitHub · Hakobi",
        html! {
            div class="min-h-screen bg-[#f5f5f7] lg:pl-60" {
                (templates::sidebar("settings", base_url, username, csrf_token))
                (templates::mobile_header(csrf_token, username))
                main class="mx-auto w-[92%] max-w-[44rem] pb-24 pt-10 lg:pt-16" {
                    p class="mb-3 text-[0.65rem] font-semibold uppercase tracking-[0.14em] text-brand-bright" { "Settings" }
                    h1 class="m-0 font-display text-[clamp(2.2rem,5vw,4rem)] font-normal tracking-[-0.04em]" { "GitHub" }
                    p class="mt-4 max-w-[40rem] leading-[1.7] text-muted" {
                        "Connect repositories for automated builds and deployments, and show your GitHub identity in the dashboard."
                    }

                    section class="mt-8 grid grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                        div class="flex items-center justify-between gap-4" {
                            div {
                                h2 class="m-0 text-[1.05rem] font-bold" { "GitHub App" }
                                @if app_installed {
                                    p class="m-0 mt-1 text-sm text-emerald-600" { "Installed — repositories are scannable." }
                                } @else {
                                    p class="m-0 mt-1 text-sm text-muted" { "Not installed yet." }
                                }
                            }
                            @if app_installed {
                                span class="rounded-full bg-emerald-50 px-3 py-1.5 text-[0.7rem] font-bold text-emerald-700" { "Connected" }
                            } @else {
                                span class="rounded-full bg-amber-50 px-3 py-1.5 text-[0.7rem] font-bold text-amber-700" { "Not connected" }
                            }
                        }
                        @if !app_installed {
                            form method="post" action="/settings/github/install" {
                                button class="rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white" type="submit" { "Install GitHub App" }
                                p class="m-0 mt-2 text-xs text-muted" {
                                    "Opens github.com to approve access. Hakobi never receives your GitHub password."
                                }
                            }
                        } @else {
                            p class="m-0 text-xs text-muted" {
                                "Manage repository access in your GitHub settings under Applications."
                            }
                        }
                    }

                    section class="mt-5 grid grid-cols-1 gap-5 rounded-[0.85rem] border border-line bg-surface/45 p-6" {
                        div class="flex items-center justify-between gap-4" {
                            div {
                                h2 class="m-0 text-[1.05rem] font-bold" { "Your GitHub identity" }
                                @if let Some(login) = github_login {
                                    p class="m-0 mt-1 text-sm text-muted" { "Signed in as " (login) }
                                } @else {
                                    p class="m-0 mt-1 text-sm text-muted" { "Connect to pick repositories in environment forms." }
                                }
                            }
                        }
                        @if github_login.is_none() && app_installed {
                            form method="post" action="/settings/github/connect" {
                                button class="rounded-lg border border-line bg-surface px-4 py-2.5 text-sm font-semibold text-ink" type="submit" { "Sign in with GitHub" }
                            }
                        }
                    }
                }
            }
        },
    )
}

#[derive(Deserialize)]
pub(crate) struct OAuthCallback {
    code: String,
    state: String,
}

/// POST /settings/github/connect — starts the user OAuth flow.
pub(crate) async fn github_connect_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(app) = catalog.load_github_app().ok().flatten() else {
        return (StatusCode::BAD_REQUEST, "GitHub App not installed").into_response();
    };
    // CSRF: random state stored in a short cookie, verified on callback.
    let mut state_bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut state_bytes);
    let oauth_state = hex::encode(state_bytes);
    let authorize = format!(
        "https://github.com/login/oauth/authorize?client_id={}&state={}",
        app.client_id, oauth_state
    );
    let cookie =
        crate::web::auth::create_cookie(&state.config, "hakobi_gh_state", &oauth_state, 600);
    let mut response = axum::response::Redirect::to(&authorize).into_response();
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());
    response
}

/// GET /settings/github/callback — exchanges the OAuth code.
pub(crate) async fn github_connect_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallback>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let cookie_state = crate::web::auth::cookie_value(&headers, "hakobi_gh_state");
    let Some(cookie_state) = cookie_state else {
        return (StatusCode::BAD_REQUEST, "missing oauth state cookie").into_response();
    };
    if cookie_state != query.state {
        return (StatusCode::FORBIDDEN, "oauth state mismatch").into_response();
    }
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(app) = catalog.load_github_app().ok().flatten() else {
        return (StatusCode::BAD_REQUEST, "GitHub App not installed").into_response();
    };
    let http = reqwest::Client::builder()
        .user_agent("hakobi-control-plane")
        .build()
        .unwrap_or_default();
    let client_id = app.client_id;
    let client_secret = app.client_secret;
    let code = query.code;
    let (token, login, uid) = match hakobi::github::GithubClient::exchange_user_code(
        &http,
        &client_id,
        &client_secret,
        &code,
    )
    .await
    {
        Ok(result) => result,
        Err(_) => return (StatusCode::BAD_GATEWAY, "GitHub exchange failed").into_response(),
    };
    if catalog
        .save_github_connection(&state.config.username, &login, uid, &token)
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    axum::response::Redirect::to("/settings/github").into_response()
}

/// DELETE /settings/github — disconnect the user's GitHub identity.
pub(crate) async fn github_disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = catalog.delete_github_connection(&state.config.username);
    axum::response::Redirect::to("/settings/github").into_response()
}

/// POST /settings/github/install — generates the App manifest and returns an
/// auto-submitting form that POSTs it to GitHub's manifest submission page
/// (GitHub requires the manifest as form fields, not a redirect).
pub(crate) async fn github_install_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let mut suffix = [0u8; 3];
    rand::rngs::OsRng.fill_bytes(&mut suffix);
    let name = format!("hakobi-{}", hex::encode(suffix));
    let base = state.config.base_url.trim_end_matches('/');
    let flow_state = hex::encode(suffix);
    let manifest = serde_json::json!({
        "name": name,
        "url": base,
        "hook_attributes": { "url": format!("{base}/webhooks/github"), "active": true },
        "redirect_url": format!("{base}/settings/github/manifest-callback"),
        "callback_urls": [format!("{base}/settings/github/callback")],
        "public": false,
        "request_oauth_on_install": true,
        "default_permissions": {
            "contents": "read",
            "statuses": "write",
            "metadata": "read",
            "pull_requests": "read"
        },
        "default_events": ["push", "pull_request"],
    });
    let cookie_value = format!(
        "{flow_state}:{}",
        serde_json::to_string(&manifest).unwrap_or_default()
    );
    let cookie =
        crate::web::auth::create_cookie(&state.config, "hakobi_gh_manifest", &cookie_value, 3600);
    // GitHub's manifest flow: form POST of `manifest` (raw JSON string) to
    // github.com/settings/apps/new with `state` in the query string.
    let page = html! {
        head {
            title { "Redirecting to GitHub…" }
        }
        body {
            form id="gh-manifest" method="post"
                action=(format!("https://github.com/settings/apps/new?state={flow_state}")) {
                input type="hidden" name="manifest" value=(manifest.to_string());
            }
            noscript {
                button type="submit" form="gh-manifest" { "Continue to GitHub" }
            }
            script { "document.getElementById('gh-manifest').submit();" }
        }
    };
    let mut response = super::html_response(StatusCode::OK, page, Vec::new());
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());
    response
}

/// GET /settings/github/manifest-callback — GitHub redirects here after the
/// admin approves, appending ?code= which converts to real credentials.
pub(crate) async fn github_manifest_post_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let Some(code) = query.get("code") else {
        return (StatusCode::BAD_REQUEST, "missing manifest code").into_response();
    };
    manifest_exchange_inner(&state, code).await
}
