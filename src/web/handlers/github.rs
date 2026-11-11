//! CP9 HTTP handlers: GitHub App setup, user OAuth, repo picker APIs,
//! webhook receiver, and the push→deploy pipeline.

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;

use super::super::deployment::{start_deployment, ExecutorDeps, Trigger};
use super::super::AppState;
use super::authenticated;
use hakobi::catalog::Catalog;
use hakobi::github::{
    changed_files_from_push, path_matches, verify_webhook_signature, GithubClient,
};

/// GET /api/github/status — app installed? user connected?
pub(crate) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let app_installed = catalog.github_app_installed().unwrap_or(false);
    let user = catalog
        .load_github_connection(&state.config.username)
        .ok()
        .flatten();
    Json(serde_json::json!({
        "app_installed": app_installed,
        "user_connected": user.is_some(),
        "github_login": user.map(|u| u.github_login),
    }))
    .into_response()
}

/// GET /api/github/repos — installation repos (cached).
pub(crate) async fn repos(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(client) = client_for(&state, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match client.list_repos().await {
        Ok(repos) => Json(serde_json::json!({ "repos": repos })).into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct RepoQuery {
    repo: String,
}

/// GET /api/github/branches?repo=owner/name
pub(crate) async fn branches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RepoQuery>,
) -> Response {
    let Some(client) = client_for(&state, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match client.list_branches(&query.repo).await {
        Ok(branches) => Json(serde_json::json!({ "branches": branches })).into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub(crate) struct TreeQuery {
    repo: String,
    branch: String,
}

/// GET /api/github/detect?repo=&branch= — Dockerfile detector.
pub(crate) async fn detect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TreeQuery>,
) -> Response {
    let Some(client) = client_for(&state, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match client.detect_apps(&query.repo, &query.branch).await {
        Ok(detection) => Json(serde_json::json!({
            "apps": detection.apps,
            "truncated": detection.truncated,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

/// Resolves a GitHub client when the app is installed and the session user
/// has connected their GitHub identity.
async fn client_for(state: &AppState, headers: &HeaderMap) -> Option<GithubClient> {
    if !authenticated(state, headers).await {
        return None;
    }
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return None;
    };
    let app = catalog.load_github_app().ok()??;
    let connection = catalog
        .load_github_connection(&state.config.username)
        .ok()??;
    GithubClient::new(
        app.app_id,
        app.installation_id,
        &app.private_key_pem,
        &connection.access_token,
    )
    .ok()
}

#[derive(Deserialize)]
pub(crate) struct ManifestCallback {
    code: String,
}

/// GET /webhooks/github/callback — GitHub App manifest exchange (admin).
pub(crate) async fn manifest_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ManifestCallback>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if state.config.username != "admin" {
        return StatusCode::FORBIDDEN.into_response();
    }
    manifest_exchange_inner(&state, &query.code).await
}

pub(crate) async fn manifest_exchange_inner(state: &AppState, code: &str) -> Response {
    let http = reqwest::Client::builder()
        .user_agent("hakobi-control-plane")
        .build()
        .unwrap_or_default();
    let response = http
        .post(format!(
            "https://api.github.com/app-manifests/{code}/conversions"
        ))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "hakobi-control-plane")
        .send()
        .await;
    let Ok(response) = response else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    if !response.status().is_success() {
        return StatusCode::BAD_GATEWAY.into_response();
    }
    #[derive(serde::Deserialize)]
    struct ManifestConversion {
        id: i64,
        client_id: String,
        client_secret: String,
        webhook_secret: String,
        pem: String,
    }
    let Ok(conversion) = response.json::<ManifestConversion>().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let claims = serde_json::json!({
        "iss": conversion.id.to_string(),
        "iat": now.saturating_sub(60),
        "exp": now + 600,
    });
    let Ok(key) = jsonwebtoken::EncodingKey::from_rsa_pem(conversion.pem.as_bytes()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(app_jwt) = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    ) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let installations = http
        .get("https://api.github.com/app/installations")
        .header("Authorization", format!("Bearer {app_jwt}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;
    let Ok(installations) = installations else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    #[derive(serde::Deserialize)]
    struct Installation {
        id: i64,
    }
    let Ok(list) = installations.json::<Vec<Installation>>().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let Some(installation) = list.first() else {
        return (StatusCode::BAD_REQUEST, "app not installed yet").into_response();
    };
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match catalog.save_github_app(
        conversion.id,
        &conversion.client_id,
        &conversion.client_secret,
        &conversion.webhook_secret,
        &conversion.pem,
        installation.id,
    ) {
        Ok(()) => github_connected_page().into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Renders the post-install success page shown after the manifest exchange.
pub(crate) fn github_connected_page() -> axum::response::Response {
    use crate::web::templates;
    use maud::html;
    let page = templates::page(
        "GitHub connected · Hakobi",
        html! {
            div class="min-h-screen bg-[#f5f5f7] lg:pl-60" {
                (templates::sidebar("settings", "", "admin", ""))
                main class="px-6 py-10 lg:px-10" {
                    div class="mx-auto max-w-lg rounded-[0.85rem] border border-line bg-surface p-8 text-center" {
                        div class="mx-auto grid size-12 place-items-center rounded-full bg-emerald-100 text-emerald-700" {
                            i class="fas fa-check" aria-hidden="true" {}
                        }
                        h1 class="mt-4 text-xl font-bold" { "GitHub App connected" }
                        p class="mt-2 text-sm text-muted" {
                            "Repositories can now be picked in environment forms, pushes build automatically, and deploy statuses land on commits."
                        }
                        a href="/settings" class="mt-6 inline-block rounded-lg bg-brand px-4 py-2.5 text-sm font-semibold text-white no-underline" { "Back to Settings" }
                    }
                }
            }
        },
    );
    axum::response::Html(page.into_string()).into_response()
}

/// POST /webhooks/github — the webhook receiver (HMAC-authenticated).
pub(crate) async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    const MAX_BODY: usize = 1024 * 1024;
    if body.len() > MAX_BODY {
        return (StatusCode::PAYLOAD_TOO_LARGE, "webhook payload too large").into_response();
    }
    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let Ok(mut catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(app) = catalog.load_github_app().ok().flatten() else {
        return StatusCode::NOT_IMPLEMENTED.into_response();
    };
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !verify_webhook_signature(&app.webhook_secret, &body, signature) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let is_new = catalog
        .record_webhook_delivery(&delivery_id)
        .unwrap_or(false);
    if !is_new {
        // Duplicate delivery (GitHub retry): acknowledge, do nothing.
        return StatusCode::OK.into_response();
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    match event {
        "ping" => StatusCode::OK.into_response(),
        "push" => handle_push(&state, &mut catalog, &payload).await,
        _ => StatusCode::OK.into_response(),
    }
}

/// Push pipeline: match environments, cancel in-flight, trigger deploys.
async fn handle_push(
    state: &AppState,
    catalog: &mut Catalog,
    payload: &serde_json::Value,
) -> Response {
    let Some(repo) = payload
        .get("repository")
        .and_then(|r| r["full_name"].as_str())
        .map(str::to_owned)
    else {
        return StatusCode::OK.into_response();
    };
    let branch = payload
        .get("ref")
        .and_then(|r| r.as_str())
        .and_then(|r| r.strip_prefix("refs/heads/"))
        .map(str::to_owned)
        .unwrap_or_default();
    let sha = payload
        .get("after")
        .and_then(|a| a.as_str())
        .map(str::to_owned)
        .unwrap_or_default();
    let message = payload
        .get("head_commit")
        .and_then(|h| h["message"].as_str())
        .map(str::to_owned);
    let Some(changed) = changed_files_from_push(payload) else {
        // Forced push or huge push: treat as "all files" so every env for
        // this repo+branch rebuilds (path filter passes trivially).
        return handle_push_all(state, catalog, &repo, &branch, &sha, message.as_deref()).await;
    };

    let environments = catalog.list_environments().unwrap_or_default();
    let mut deployed = 0usize;
    for environment in &environments {
        let Some(env_repo) = environment.github_repo.as_deref() else {
            continue;
        };
        if env_repo != repo || environment.github_branch.as_deref() != Some(branch.as_str()) {
            continue;
        }
        if !environment.github_auto_deploy {
            continue;
        }
        let pattern = environment.github_path.as_deref().unwrap_or("");
        if !path_matches(pattern, &changed) {
            continue;
        }
        deploy_environment(
            state,
            catalog,
            environment.id.as_str(),
            &sha,
            message.as_deref(),
        );
        deployed += 1;
    }
    Json(serde_json::json!({ "deployments_triggered": deployed })).into_response()
}

/// No changed-file set (forced push / big push): rebuild every matching env.
async fn handle_push_all(
    state: &AppState,
    catalog: &mut Catalog,
    repo: &str,
    branch: &str,
    sha: &str,
    message: Option<&str>,
) -> Response {
    let environments = catalog.list_environments().unwrap_or_default();
    let mut deployed = 0usize;
    for environment in &environments {
        if environment.github_repo.as_deref() != Some(repo)
            || environment.github_branch.as_deref() != Some(branch)
            || !environment.github_auto_deploy
        {
            continue;
        }
        deploy_environment(state, catalog, environment.id.as_str(), sha, message);
        deployed += 1;
    }
    Json(serde_json::json!({ "deployments_triggered": deployed })).into_response()
}

/// Triggers one deployment; marks any in-flight deployment for the same
/// environment as cancelled (newest push wins).
fn deploy_environment(
    state: &AppState,
    catalog: &mut Catalog,
    environment_id: &str,
    sha: &str,
    message: Option<&str>,
) {
    if let Ok(Some(_in_flight)) = catalog.latest_in_flight_deployment(environment_id) {
        // Newest push wins: mark superseded builds so the UI reflects it.
        let _ = catalog.cancel_in_flight_deployments(environment_id);
    }
    let deps = ExecutorDeps {
        catalog_path: (*state.catalog_path).clone(),
        build_runner: state.build_runner.clone(),
        backend: state.backend.clone(),
    };
    let guard = state.deployments.clone();
    let _ = start_deployment(
        &deps,
        &guard,
        environment_id,
        Trigger::Webhook,
        Some(sha),
        message,
    );
}
