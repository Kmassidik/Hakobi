use std::sync::{mpsc, Arc};

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, header::COOKIE, header::ORIGIN, Request, StatusCode},
    Router,
};
use hakobi::{buildrunner::BuildRunnerClient, catalog::Catalog};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tower::ServiceExt;

use super::router_with_pipeline;
use super::tests::{config, sign_in, PipelineBackend};
use super::tests_support::{app, body_text, form_encode, post_form};

const PUBLIC_URL: &str = "https://gitlab.com/acme/public-app.git";

fn create_host(path: &std::path::Path) -> String {
    Catalog::open(path)
        .unwrap()
        .create_host_agent(
            "public-source-host",
            "https://public-source.example.com",
            "tok_public_source_01",
        )
        .unwrap()
}

fn public_form(host_id: &str, csrf: &str, name: &str, url: &str, branch: &str) -> String {
    format!(
        "name={name}&host_id={host_id}&build_type=dockerfile&source_kind=public&github_repo={}&github_branch={}&github_auto_deploy=1&github_dockerfile=Dockerfile&cpu_limit=1&memory_limit_mb=512&csrf_token={csrf}",
        form_encode(url),
        form_encode(branch),
    )
}

async fn get_page(app: &Router, authenticated: &str, uri: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(
            Request::get(uri)
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    (response.status(), body_text(response, 128 * 1024).await)
}

async fn create_public_app(app: &Router, authenticated: &str, csrf: &str, host_id: &str) -> String {
    let (status, page) = get_page(app, authenticated, "/apps/new").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Your repositories"));
    assert!(page.contains("Public repository"));
    assert!(page.contains("data-public-repo"));
    assert!(page.contains("data-public-repo-scan"));
    assert!(page.contains("data-public-ref-panel hidden"));
    let response = post_form(
        app,
        "/apps",
        authenticated,
        "null",
        public_form(host_id, csrf, "public-web", PUBLIC_URL, "v1.2.3"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .split('/')
        .nth(2)
        .unwrap()
        .split('?')
        .next()
        .unwrap()
        .to_owned()
}

async fn assert_public_edit_form(app: &Router, authenticated: &str, app_id: &str) {
    let (status, page) = get_page(app, authenticated, &format!("/apps/{app_id}/edit")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("name=\"source_kind\""));
    assert!(page.contains("data-source-card=\"public\" data-state=\"selected\""));
    assert!(page.contains("data-source-card=\"connected\" data-state=\"unselected\""));
    assert!(page.contains(PUBLIC_URL));
    assert!(page.contains("value=\"v1.2.3\""));
    assert!(page.contains("Manual deployments only"));
}

async fn update_public_app(
    app: &Router,
    path: &std::path::Path,
    authenticated: &str,
    csrf: &str,
    host_id: &str,
    app_id: &str,
) {
    let updated_url = "https://codeberg.org/acme/updated.git";
    let response = post_form(
        app,
        &format!("/apps/{app_id}/edit"),
        authenticated,
        "null",
        public_form(host_id, csrf, "public-web-2", updated_url, "release-2026"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(path).unwrap();
    let updated = catalog.get_app(app_id).unwrap().unwrap();
    assert_eq!(updated.name, "public-web-2");
    assert_eq!(updated.github_repo.as_deref(), Some(updated_url));
    assert_eq!(updated.github_branch.as_deref(), Some("release-2026"));
    assert!(!updated.github_auto_deploy);
}

#[tokio::test]
async fn public_url_create_and_edit_round_trip() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let host_id = create_host(&path);
    let app_id = create_public_app(&app, &authenticated, &csrf, &host_id).await;
    let catalog = Catalog::open(&path).unwrap();
    let created = catalog.get_app(&app_id).unwrap().unwrap();
    assert_eq!(created.github_repo.as_deref(), Some(PUBLIC_URL));
    assert_eq!(created.github_branch.as_deref(), Some("v1.2.3"));
    assert!(!created.github_auto_deploy);
    drop(catalog);
    let (_, detail) = get_page(&app, &authenticated, &format!("/apps/{app_id}")).await;
    assert!(detail.contains("Deploy latest"));
    assert!(detail.contains("Manual deploys only"));
    assert_public_edit_form(&app, &authenticated, &app_id).await;
    update_public_app(&app, &path, &authenticated, &csrf, &host_id, &app_id).await;
}

#[tokio::test]
async fn connected_source_submission_is_unchanged() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let host_id = create_host(&path);
    let response = post_form(
        &app,
        "/apps",
        &authenticated,
        "null",
        format!("name=connected-web&host_id={host_id}&build_type=dockerfile&source_kind=connected&github_repo=acme%2Fapp&github_branch=main&github_auto_deploy=1&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(&path).unwrap();
    let created = catalog.list_apps().unwrap().remove(0);
    assert_eq!(created.github_repo.as_deref(), Some("acme/app"));
    assert_eq!(created.github_branch.as_deref(), Some("main"));
    assert!(created.github_auto_deploy);
}

#[tokio::test]
async fn https_urls_cannot_bypass_validation_as_connected_sources() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let host_id = create_host(&path);
    let response = post_form(
        &app,
        "/apps",
        &authenticated,
        "null",
        format!("name=invalid-public&host_id={host_id}&build_type=dockerfile&source_kind=connected&github_repo=https%3A%2F%2Fuser%40github.com%2Facme%2Fapp.git&github_branch=main&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(Catalog::open(&path)
        .unwrap()
        .list_apps()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn invalid_public_urls_are_rejected_over_http() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let host_id = create_host(&path);
    for url in [
        "",
        "http://github.com/acme/app.git",
        "ssh://git@github.com/acme/app.git",
        "git@github.com:acme/app.git",
        "https://user@github.com/acme/app.git",
        "https://user:secret@github.com/acme/app.git",
        "https://github.com/acme/app.git?token=secret",
        "https://github.com/acme/app.git#main",
    ] {
        let response = post_form(
            &app,
            "/apps",
            &authenticated,
            "null",
            public_form(&host_id, &csrf, "invalid-public", url, "main"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "URL: {url}");
        let page = body_text(response, 128 * 1024).await;
        assert!(page.contains("Public repository URL must be HTTPS"));
    }
    assert!(Catalog::open(&path)
        .unwrap()
        .list_apps()
        .unwrap()
        .is_empty());
}

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

fn seed_app(path: &std::path::Path, name: &str, repo: &str, automatic: bool) -> String {
    let mut catalog = Catalog::open(path).unwrap();
    let host_id = catalog
        .create_host_agent(
            &format!("{name}-host"),
            &format!("https://{name}.example.com"),
            &format!("tok_{name}_01"),
        )
        .unwrap();
    catalog
        .create_app(
            name,
            &host_id,
            "dockerfile",
            Some(repo),
            Some("main"),
            None,
            Some("Dockerfile"),
            automatic,
            Some(3000),
            None,
            None,
            None,
        )
        .unwrap()
}

fn capturing_runner() -> (BuildRunnerClient, mpsc::Receiver<serde_json::Value>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stream = listener.incoming().next().unwrap().unwrap();
        let mut bytes = Vec::new();
        let captured = loop {
            let mut chunk = [0_u8; 4096];
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0, "connection closed before a complete request");
            bytes.extend_from_slice(&chunk[..read]);
            let request = String::from_utf8_lossy(&bytes);
            if let Some(body) = request.split("\r\n\r\n").nth(1) {
                if let Ok(captured) = serde_json::from_str::<serde_json::Value>(body) {
                    break captured;
                }
            }
        };
        sender.send(captured).unwrap();
        let payload = r#"{"status":"succeeded","image_id":"sha256:public","exit_code":0,"duration_ms":5,"log_tail":"ok","truncated":false,"detail":null}"#;
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
        stream.write_all(response.as_bytes()).unwrap();
    });
    (
        BuildRunnerClient::new(&format!("http://{address}"), None).unwrap(),
        receiver,
    )
}

async fn await_terminal(path: &std::path::Path, app_id: &str) {
    for _ in 0..500 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let catalog = Catalog::open(path).unwrap();
        if let Some(deployment) = catalog.get_latest_deployment(app_id).unwrap() {
            if deployment.status == "running" || deployment.status == "failed" {
                assert_eq!(deployment.status, "running");
                return;
            }
        }
    }
    panic!("deployment never reached a terminal state");
}

#[tokio::test]
async fn public_deployment_sends_the_unmodified_clone_url() {
    let (runner, request) = capturing_runner();
    let (data, app) = pipeline_app(Some(runner));
    let path = data.path().join("catalog.sqlite3");
    let app_id = seed_app(&path, "public-deploy", PUBLIC_URL, false);
    let (authenticated, csrf) = sign_in(&app).await;
    let response = post_form(
        &app,
        &format!("/apps/{app_id}/deploy"),
        &authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    await_terminal(&path, &app_id).await;
    let payload = request
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert_eq!(payload["git_url"], PUBLIC_URL);
    assert_eq!(payload["branch"], "main");
    assert!(payload.get("token").is_none());
}

fn signed_webhook(payload: &'static str) -> Request<Body> {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(b"github-webhook-secret").unwrap();
    mac.update(payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    Request::post("/webhooks/github")
        .header(CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", "public-source-non-match")
        .header("x-hub-signature-256", format!("sha256={signature}"))
        .body(Body::from(payload))
        .unwrap()
}

#[tokio::test]
async fn github_push_does_not_match_public_url_apps() {
    let (data, app) = pipeline_app(None);
    let path = data.path().join("catalog.sqlite3");
    let connected_id = seed_app(&path, "connected", "acme/app", true);
    let public_id = seed_app(&path, "public", "https://github.com/acme/app.git", true);
    let catalog = Catalog::open(&path).unwrap();
    catalog
        .save_github_app(
            1,
            "hakobi-test",
            "client-id",
            "client-secret",
            "github-webhook-secret",
            "unused-private-key",
        )
        .unwrap();
    catalog.set_github_installation(42).unwrap();
    drop(catalog);
    let payload = r#"{"ref":"refs/heads/main","after":"abc123","repository":{"full_name":"acme/app"},"commits":[{"added":[],"modified":["src/main.rs"],"removed":[]}]}"#;
    let response = app.oneshot(signed_webhook(payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_text(response, 4096)
        .await
        .contains("deployments_triggered\":1"));
    let catalog = Catalog::open(&path).unwrap();
    assert_eq!(catalog.list_deployments(&connected_id).unwrap().len(), 1);
    assert!(catalog.list_deployments(&public_id).unwrap().is_empty());
}

#[tokio::test]
async fn public_scan_requires_authentication_csrf_and_an_enabled_host() {
    let (_data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let body = serde_json::json!({
        "host_id": "missing", "git_url": "https://github.com/acme/app",
        "branch": null, "csrf_token": csrf,
    })
    .to_string();
    let forbidden = app
        .clone()
        .oneshot(
            Request::post("/api/public/scan")
                .header(COOKIE, &authenticated)
                .header(CONTENT_TYPE, "application/json")
                .header(ORIGIN, "https://evil.example")
                .body(Body::from(body.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
    let invalid_host = app
        .oneshot(
            Request::post("/api/public/scan")
                .header(COOKIE, authenticated)
                .header(CONTENT_TYPE, "application/json")
                .header(ORIGIN, "http://127.0.0.1:3000")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_host.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn public_environment_requires_a_session_and_rejects_unsafe_urls() {
    let (_data, app) = app();
    let unauthenticated = app
        .clone()
        .oneshot(
            Request::get(
                "/api/public/environment?repo=https%3A%2F%2Fgithub.com%2Facme%2Fapp&branch=main",
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    let (authenticated, _) = sign_in(&app).await;
    let invalid = app
        .oneshot(
            Request::get(
                "/api/public/environment?repo=https%3A%2F%2F127.0.0.1%2Facme%2Fapp&branch=main",
            )
            .header(COOKIE, authenticated)
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
}
