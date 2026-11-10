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

fn seed_environment(path: &std::path::Path, secret: Option<&str>) -> (String, String) {
    let mut catalog = hakobi::catalog::Catalog::open(path).unwrap();
    let host_id = catalog
        .create_host_agent("deploy-host", "https://deploy.example.com", "tok_deploy_01")
        .unwrap();
    let environment_id = catalog
        .create_environment(
            "web",
            &host_id,
            "dockerfile",
            "git",
            Some("https://github.com/acme/app.git"),
            Some("main"),
            "local",
        )
        .unwrap();
    if let Some(secret) = secret {
        catalog
            .set_environment_webhook(&environment_id, Some(secret), true)
            .unwrap();
    }
    (host_id, environment_id)
}

/// Waits until the environment's latest deployment reaches a terminal state.
async fn await_terminal(path: &std::path::Path, environment_id: &str) -> String {
    for _ in 0..200 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let catalog = hakobi::catalog::Catalog::open(path).unwrap();
        if let Some(deployment) = catalog.get_latest_deployment(environment_id).unwrap() {
            if deployment.status == "running" || deployment.status == "failed" {
                return deployment.status;
            }
        }
    }
    panic!("deployment never reached a terminal state");
}

#[tokio::test]
async fn webhook_rejects_bad_secrets_and_accepts_valid_payloads() {
    let runner = scripted_runner(PipelineScript::Ok {
        image_id: "sha256:abc123".to_owned(),
        exit_code: 0,
    });
    let (data, app) = pipeline_app(Some(runner));
    let path = data.path().join("catalog.sqlite3");
    let (_host_id, environment_id) = seed_environment(&path, Some("whsec_test_secret_123"));
    let payload = r#"{"ref":"refs/heads/main","head_commit":{"id":"abc123def456","message":"fix: login bug"}}"#;

    // Wrong secret: 403.
    let forbidden = app
        .clone()
        .oneshot(
            Request::post(format!("/webhook/{environment_id}/definitely-wrong-secret"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // Unknown environment: 404.
    let missing = app
        .clone()
        .oneshot(
            Request::post("/webhook/no-such-env/whsec_test_secret_123")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // Disabled webhook (secret set but disabled) is 404 too.
    {
        let mut catalog = hakobi::catalog::Catalog::open(&path).unwrap();
        catalog
            .set_environment_webhook(&environment_id, Some("whsec_test_secret_123"), false)
            .unwrap();
    }
    let disabled = app
        .clone()
        .oneshot(
            Request::post(format!("/webhook/{environment_id}/whsec_test_secret_123"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(disabled.status(), StatusCode::NOT_FOUND);
    {
        let mut catalog = hakobi::catalog::Catalog::open(&path).unwrap();
        catalog
            .set_environment_webhook(&environment_id, Some("whsec_test_secret_123"), true)
            .unwrap();
    }

    // Valid secret: 202 with a queued deployment.
    let accepted = app
        .clone()
        .oneshot(
            Request::post(format!("/webhook/{environment_id}/whsec_test_secret_123"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(payload))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    let body = to_bytes(accepted.into_body(), 16 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("queued"), "{body}");

    let status = await_terminal(&path, &environment_id).await;
    assert_eq!(status, "running");
    let catalog = hakobi::catalog::Catalog::open(&path).unwrap();
    let deployment = catalog
        .get_latest_deployment(&environment_id)
        .unwrap()
        .unwrap();
    assert_eq!(deployment.trigger_type, "webhook");
    assert_eq!(deployment.git_commit_sha.as_deref(), Some("abc123def456"));
    assert_eq!(
        deployment.git_commit_message.as_deref(),
        Some("fix: login bug")
    );

    // GitHub ping: accepted without creating a deployment.
    let before = {
        let catalog = hakobi::catalog::Catalog::open(&path).unwrap();
        catalog.list_deployments(&environment_id).unwrap().len()
    };
    let ping = app
        .clone()
        .oneshot(
            Request::post(format!("/webhook/{environment_id}/whsec_test_secret_123"))
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"zen":"Design is not just how it looks."}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ping.status(), StatusCode::ACCEPTED);
    let after = {
        let catalog = hakobi::catalog::Catalog::open(&path).unwrap();
        catalog.list_deployments(&environment_id).unwrap().len()
    };
    assert_eq!(before, after, "ping must not create a deployment");
}

#[tokio::test]
async fn deployment_page_lists_history_with_badges_and_htmx() {
    let (data, app) = pipeline_app(None);
    let path = data.path().join("catalog.sqlite3");
    let (_host_id, environment_id) = seed_environment(&path, None);
    let (authenticated, _csrf) = sign_in(&app).await;

    {
        let mut catalog = hakobi::catalog::Catalog::open(&path).unwrap();
        let v1 = catalog
            .create_deployment(&environment_id, "manual", None, None)
            .unwrap();
        catalog
            .update_deployment_status(&v1, "running", None)
            .unwrap();
        let v2 = catalog
            .create_deployment(&environment_id, "webhook", Some("abc123"), None)
            .unwrap();
        catalog
            .update_deployment_status(&v2, "failed", Some("boom"))
            .unwrap();
        let v3 = catalog
            .create_deployment(&environment_id, "manual", None, None)
            .unwrap();
        catalog
            .update_deployment_status(&v3, "rolled_back", None)
            .unwrap();
    }

    let page = app
        .clone()
        .oneshot(
            Request::get("/deployment")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = super::tests::body_text(page, 256 * 1024).await;
    assert!(body.contains("Deployments"));
    assert!(body.contains(&format!("/environments/{environment_id}/deploy")));
    assert!(body.contains(&format!("/environments/{environment_id}/rollback")));
    assert!(body.contains("every 5s"));
    assert!(body.contains("running"));
    assert!(body.contains("failed"));
    assert!(body.contains("rolled_back"));
    assert!(body.contains("abc123"));
    assert!(body.contains("Webhook"));
    assert!(body.contains(&format!("/environments/{environment_id}/webhook")));

    // Unauthenticated access redirects to login.
    let anonymous = app
        .clone()
        .oneshot(Request::get("/deployment").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn webhook_config_page_saves_secret_and_toggle() {
    let (data, app) = pipeline_app(None);
    let path = data.path().join("catalog.sqlite3");
    let (_host_id, environment_id) = seed_environment(&path, None);
    let (authenticated, csrf) = sign_in(&app).await;

    let page = app
        .clone()
        .oneshot(
            Request::get(format!("/environments/{environment_id}/webhook"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = super::tests::body_text(page, 128 * 1024).await;
    assert!(body.contains("Webhook secret"));
    assert!(body.contains(&format!("/environments/{environment_id}/webhook")));

    let save = super::tests::post_form(
        &app,
        &format!("/environments/{environment_id}/webhook"),
        &authenticated,
        "null",
        format!("webhook_secret=whsec_new_123&webhook_enabled=on&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(save.status(), StatusCode::SEE_OTHER);
    let catalog = hakobi::catalog::Catalog::open(&path).unwrap();
    let environment = catalog.get_environment(&environment_id).unwrap().unwrap();
    assert_eq!(environment.webhook_secret.as_deref(), Some("whsec_new_123"));
    assert!(environment.webhook_enabled);
}
