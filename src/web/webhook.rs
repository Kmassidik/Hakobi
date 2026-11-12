//! Webhook receiver: `POST /webhook/{app_id}/{secret}` triggers a
//! deployment after verifying the secret and parsing the commit metadata.
//!
//! GitHub and GitLab both POST JSON; we accept either payload shape plus a
//! bare `{"commit_sha": "…"}` for simple CI systems. Authentication is the
//! URL secret compared against the app's `webhook_secret` in
//! constant time.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use subtle::ConstantTimeEq;

use super::deployment::{start_deployment, ExecutorDeps, Trigger};
use super::AppState;
use crate::web::handlers::{internal_error, with_catalog};

/// Maximum accepted webhook body. Commit payloads are small.
const MAX_WEBHOOK_BODY: usize = 64 * 1024;

/// A parsed commit reference from any supported payload shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommitRef {
    pub sha: Option<String>,
    pub message: Option<String>,
}

/// Union of the webhook payload shapes Hakobi accepts.
#[derive(Debug, Deserialize)]
struct WebhookPayload {
    #[serde(default)]
    head_commit: Option<HeadCommit>,
    #[serde(default)]
    commit_sha: Option<String>,
    #[serde(default)]
    commit_message: Option<String>,
    // GitLab push events carry the commit id at the top level.
    #[serde(default)]
    after: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HeadCommit {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Extracts the commit SHA and message from a webhook payload.
///
/// GitHub send `head_commit`; GitLab pushes `after`; simple CI systems may
/// post `commit_sha` directly. GitHub's ping event (a bare `zen`) carries no
/// commit and is acknowledged without deploying.
pub(crate) fn parse_commit(payload: &str) -> CommitRef {
    let parsed: Option<WebhookPayload> = serde_json::from_str(payload).ok();
    match parsed {
        None => CommitRef {
            sha: None,
            message: None,
        },
        Some(payload) => {
            if let Some(head) = payload.head_commit {
                return CommitRef {
                    sha: head.id.filter(|id| !id.trim().is_empty()),
                    message: head.message.filter(|m| !m.trim().is_empty()),
                };
            }
            let sha = payload.commit_sha.or(payload.after).filter(|sha| {
                !sha.trim().is_empty() && sha.trim() != "0000000000000000000000000000000000000000"
            });
            CommitRef {
                sha,
                message: payload.commit_message.filter(|m| !m.trim().is_empty()),
            }
        }
    }
}

/// Constant-time comparison of the submitted secret with the stored one.
pub(crate) fn secret_matches(expected: &str, submitted: &str) -> bool {
    let expected = expected.as_bytes();
    let submitted = submitted.as_bytes();
    expected.len() == submitted.len() && expected.ct_eq(submitted).into()
}

/// The webhook receiver endpoint. Unauthenticated by design: the URL secret
/// is the credential, so no session cookie or CSRF token applies.
pub(crate) async fn webhook_receive(
    State(state): State<AppState>,
    Path((app_id, secret)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Response {
    if body.len() > MAX_WEBHOOK_BODY {
        return (StatusCode::PAYLOAD_TOO_LARGE, "webhook payload too large").into_response();
    }
    let app_key = app_id.clone();
    let submitted_secret = secret.clone();
    let decision = match with_catalog(&state, move |catalog| {
        let Some(app) = catalog.get_app(&app_key)? else {
            return Ok(WebhookDecision::UnknownApp);
        };
        Ok(decide_webhook(
            &app.webhook_secret,
            app.webhook_enabled,
            &submitted_secret,
        ))
    })
    .await
    {
        Ok(decision) => decision,
        Err(error) => return internal_error(error),
    };
    match decision {
        WebhookDecision::UnknownApp | WebhookDecision::Disabled => {
            return StatusCode::NOT_FOUND.into_response();
        }
        WebhookDecision::BadSecret => return StatusCode::FORBIDDEN.into_response(),
        WebhookDecision::Accepted => {}
    }

    let payload = String::from_utf8_lossy(&body);
    let commit = parse_commit(&payload);
    // Payloads without a commit (GitHub's ping, custom pings) are
    // acknowledged without deploying.
    let Some(sha) = commit.sha else {
        return (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"status": "acknowledged"})),
        )
            .into_response();
    };

    let deps = ExecutorDeps {
        catalog_path: (*state.catalog_path).clone(),
        build_runner: state.build_runner.clone(),
        backend: state.backend.clone(),
    };
    let guard = state.deployments.clone();
    let message = commit.message.as_deref();
    match start_deployment(
        &deps,
        &guard,
        &app_id,
        Trigger::Webhook,
        Some(&sha),
        message,
    ) {
        Ok(Some(deployment_id)) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"status": "queued", "deployment_id": deployment_id})),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "a deployment is already running"})),
        )
            .into_response(),
        Err(error) => internal_error(error),
    }
}

/// What the receiver should do after checking the app row.
enum WebhookDecision {
    Accepted,
    UnknownApp,
    Disabled,
    BadSecret,
}

/// Pure decision function so the secret logic is unit-testable without HTTP.
fn decide_webhook(
    stored_secret: &Option<String>,
    webhook_enabled: bool,
    submitted: &str,
) -> WebhookDecision {
    let Some(expected) = stored_secret.as_deref().filter(|s| !s.is_empty()) else {
        return WebhookDecision::Disabled;
    };
    if !webhook_enabled {
        return WebhookDecision::Disabled;
    }
    if secret_matches(expected, submitted) {
        WebhookDecision::Accepted
    } else {
        WebhookDecision::BadSecret
    }
}
