//! CP6 webhook receiver and deployment UI tests.

use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header::CONTENT_TYPE, header::COOKIE, Request, StatusCode},
    Router,
};
use hakobi::buildrunner::BuildRunnerClient;
use tower::ServiceExt;

use super::router_with_pipeline;
use super::tests::{config, sign_in, PipelineBackend};
use super::tests_pipeline_support::{start_scripted_runner, PipelineScript};

fn pipeline_app(build_runner: Option<BuildRunnerClient>) -> (tempfile::TempDir, Router) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.sqlite3");
    let app = router_with_pipeline(
        config(),
        path,
        Arc::new(PipelineBackend::new()),
        build_runner,
    );
    (directory, app.expect("router build"))
}

fn scripted_runner(script: PipelineScript) -> BuildRunnerClient {
    let base_url = start_scripted_runner(script, 0);
    BuildRunnerClient::new(&base_url, None).unwrap()
}

fn seed_app(path: &std::path::Path, secret: Option<&str>) -> (String, String) {
    let mut catalog = hakobi::catalog::Catalog::open(path).unwrap();
    let host_id = catalog
        .create_host_agent("deploy-host", "https://deploy.example.com", "tok_deploy_01")
        .unwrap();
    let app_id = catalog
        .create_app(
            "web",
            &host_id,
            "dockerfile",
            Some("acme/app"),
            Some("main"),
            None,
            Some("Dockerfile"),
            false,
            Some(3000),
        )
        .unwrap();
    if let Some(secret) = secret {
        catalog
            .set_app_webhook(&app_id, Some(secret), true)
            .unwrap();
    }
    (host_id, app_id)
}

/// Waits until the app's latest deployment reaches a terminal state.
async fn await_terminal(path: &std::path::Path, app_id: &str) -> String {
    for _ in 0..200 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let catalog = hakobi::catalog::Catalog::open(path).unwrap();
        if let Some(deployment) = catalog.get_latest_deployment(app_id).unwrap() {
            if deployment.status == "running" || deployment.status == "failed" {
                return deployment.status;
            }
        }
    }
    panic!("deployment never reached a terminal state");
}

async fn post_webhook(
    app: &Router,
    app_id: &str,
    secret: &str,
    payload: &'static str,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::post(format!("/webhook/{app_id}/{secret}"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap()
}

fn set_webhook_enabled(path: &std::path::Path, app_id: &str, enabled: bool) {
    let mut catalog = hakobi::catalog::Catalog::open(path).unwrap();
    catalog
        .set_app_webhook(app_id, Some("whsec_test_secret_123"), enabled)
        .unwrap();
}

fn deployment_count(path: &std::path::Path, app_id: &str) -> usize {
    hakobi::catalog::Catalog::open(path)
        .unwrap()
        .list_deployments(app_id)
        .unwrap()
        .len()
}

async fn assert_ping_does_not_create_deployment(
    app: &Router,
    path: &std::path::Path,
    app_id: &str,
) {
    let before = deployment_count(path, app_id);
    let ping = post_webhook(
        app,
        app_id,
        "whsec_test_secret_123",
        r#"{"zen":"Design is not just how it looks."}"#,
    )
    .await;
    assert_eq!(ping.status(), StatusCode::ACCEPTED);
    let after = deployment_count(path, app_id);
    assert_eq!(before, after, "ping must not create a deployment");
}

#[tokio::test]
async fn webhook_rejects_bad_secrets_and_accepts_valid_payloads() {
    let runner = scripted_runner(PipelineScript::Ok {
        image_id: "sha256:abc123".to_owned(),
        exit_code: 0,
    });
    let (data, app) = pipeline_app(Some(runner));
    let path = data.path().join("catalog.sqlite3");
    let (_host_id, app_id) = seed_app(&path, Some("whsec_test_secret_123"));
    let payload = r#"{"ref":"refs/heads/main","head_commit":{"id":"abc123def456","message":"fix: login bug"}}"#;

    // Wrong secret: 403.
    let forbidden = post_webhook(&app, &app_id, "definitely-wrong-secret", payload).await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // Unknown app: 404.
    let missing = post_webhook(&app, "no-such-env", "whsec_test_secret_123", payload).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // Disabled webhook (secret set but disabled) is 404 too.
    set_webhook_enabled(&path, &app_id, false);
    let disabled = post_webhook(&app, &app_id, "whsec_test_secret_123", payload).await;
    assert_eq!(disabled.status(), StatusCode::NOT_FOUND);
    set_webhook_enabled(&path, &app_id, true);

    // Valid secret: 202 with a queued deployment.
    let accepted = post_webhook(&app, &app_id, "whsec_test_secret_123", payload).await;
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let body = to_bytes(accepted.into_body(), 16 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("queued"), "{body}");

    let status = await_terminal(&path, &app_id).await;
    assert_eq!(status, "running");
    let catalog = hakobi::catalog::Catalog::open(&path).unwrap();
    let deployment = catalog.get_latest_deployment(&app_id).unwrap().unwrap();
    assert_eq!(deployment.trigger_type, "webhook");
    assert_eq!(deployment.git_commit_sha.as_deref(), Some("abc123def456"));
    assert_eq!(
        deployment.git_commit_message.as_deref(),
        Some("fix: login bug")
    );

    // GitHub ping: accepted without creating a deployment.
    assert_ping_does_not_create_deployment(&app, &path, &app_id).await;
}

#[tokio::test]
async fn app_detail_lists_history_with_badges_and_htmx() {
    let (data, app) = pipeline_app(None);
    let path = data.path().join("catalog.sqlite3");
    let (_host_id, app_id) = seed_app(&path, Some("whsec_test_secret_123"));
    let (authenticated, _csrf) = sign_in(&app).await;
    {
        let mut catalog = hakobi::catalog::Catalog::open(&path).unwrap();
        let deployment = catalog
            .create_deployment(
                &app_id,
                "webhook",
                Some("abc123def456"),
                Some("fix: login bug"),
            )
            .unwrap();
        catalog
            .update_deployment_status(&deployment, "running", None)
            .unwrap();
    }

    let page = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = super::tests::body_text(page, 256 * 1024).await;
    assert!(body.contains("Deployments"));
    assert!(body.contains(&format!("/apps/{app_id}/deploy")));
    assert!(body.contains("abc123def456"));
    assert!(body.contains("/webhook/"));

    // Unauthenticated access redirects to login.
    let anonymous = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);
}
#[tokio::test]
async fn detail_shows_webhook_url_when_secret_configured() {
    let (data, app) = pipeline_app(None);
    let path = data.path().join("catalog.sqlite3");
    let (_host_id, app_id) = seed_app(&path, Some("whsec_test_secret_123"));
    let (authenticated, _csrf) = sign_in(&app).await;

    let detail = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    let body = super::tests::body_text(detail, 128 * 1024).await;
    assert!(body.contains(&format!("/webhook/{app_id}/whsec_test_secret_123")));
}
