use std::path::Path;

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::Response,
    Router,
};
use hakobi::catalog::Catalog;
use tower::ServiceExt;

use super::tests_support::{app, body_text, sign_in};

fn seed_app(path: &Path, name: &str) -> (String, String) {
    let mut catalog = Catalog::open(path).unwrap();
    let host = catalog
        .create_host_agent(
            &format!("terminal-host-{name}"),
            &format!("https://{name}.example.com"),
            &format!("tok_terminal_{name}"),
        )
        .unwrap();
    let app = catalog
        .create_app(
            name,
            &host,
            "dockerfile",
            Some("acme/web"),
            Some("main"),
            None,
            Some("Dockerfile"),
            false,
            Some(3000),
            None,
            None,
            None,
        )
        .unwrap();
    let deployment = catalog
        .create_deployment(&app, "manual", None, None)
        .unwrap();
    (app, deployment)
}

async fn get(router: &Router, uri: &str, cookies: Option<&str>) -> Response {
    let mut request = Request::get(uri);
    if let Some(cookies) = cookies {
        request = request.header(header::COOKIE, cookies);
    }
    router
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn terminal_routes_require_authentication_and_known_deployments() {
    let (directory, router) = app();
    let (_, deployment) = seed_app(&directory.path().join("catalog.sqlite3"), "web");
    let uri = format!("/deployments/{deployment}/terminal");
    assert_eq!(
        get(&router, &uri, None).await.status(),
        StatusCode::SEE_OTHER
    );
    let events = format!("/deployments/{deployment}/events");
    assert_eq!(
        get(&router, &events, None).await.status(),
        StatusCode::SEE_OTHER
    );

    let (cookies, _) = sign_in(&router).await;
    let missing = get(&router, "/deployments/missing/terminal", Some(&cookies)).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing = get(&router, "/deployments/missing/events", Some(&cookies)).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn terminal_partial_and_completed_event_stream_replay_then_close() {
    let (directory, router) = app();
    let path = directory.path().join("catalog.sqlite3");
    let (_, deployment) = seed_app(&path, "web");
    let mut catalog = Catalog::open(&path).unwrap();
    catalog
        .add_deployment_log(&deployment, "info", "clone \"done\"")
        .unwrap();
    catalog
        .update_deployment_status(&deployment, "failed", None)
        .unwrap();
    drop(catalog);
    let (cookies, _) = sign_in(&router).await;

    let partial = get(
        &router,
        &format!("/deployments/{deployment}/terminal"),
        Some(&cookies),
    )
    .await;
    assert_eq!(partial.status(), StatusCode::OK);
    let html = body_text(partial, 128 * 1024).await;
    assert!(html.contains("data-build-terminal"));
    assert!(html.contains("clone"));

    let response = get(
        &router,
        &format!("/deployments/{deployment}/events"),
        Some(&cookies),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()["x-accel-buffering"], "no");
    let body = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        body_text(response, 128 * 1024),
    )
    .await
    .expect("terminal stream must close");
    assert!(body.contains("event: status"));
    assert!(body.contains("\"status\":\"failed\""));
    assert!(body.contains("event: log"));
    assert!(body.contains("clone \\\"done\\\""));
}

#[tokio::test]
async fn event_stream_emits_new_logs_and_final_status() {
    let (directory, router) = app();
    let path = directory.path().join("catalog.sqlite3");
    let (_, deployment) = seed_app(&path, "web");
    let mut catalog = Catalog::open(&path).unwrap();
    catalog
        .add_deployment_log(&deployment, "info", "first")
        .unwrap();
    catalog
        .update_deployment_status(&deployment, "building", None)
        .unwrap();
    drop(catalog);
    let (cookies, _) = sign_in(&router).await;
    let response = get(
        &router,
        &format!("/deployments/{deployment}/events"),
        Some(&cookies),
    )
    .await;
    let reader = tokio::spawn(body_text(response, 128 * 1024));
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    let mut catalog = Catalog::open(&path).unwrap();
    catalog
        .add_deployment_log(&deployment, "info", "second")
        .unwrap();
    catalog
        .update_deployment_status(&deployment, "running", None)
        .unwrap();
    drop(catalog);
    let body = tokio::time::timeout(std::time::Duration::from_secs(3), reader)
        .await
        .expect("stream closed after running")
        .unwrap();
    assert!(body.contains("first"));
    assert!(body.contains("second"));
    assert!(body.contains("\"status\":\"building\""));
    assert!(body.contains("\"status\":\"running\""));
}

#[tokio::test]
async fn app_logs_selects_only_deployments_from_the_same_app() {
    let (directory, router) = app();
    let path = directory.path().join("catalog.sqlite3");
    let (app_id, selected) = seed_app(&path, "web");
    let (_, other) = seed_app(&path, "other");
    let mut catalog = Catalog::open(&path).unwrap();
    catalog
        .add_deployment_log(&selected, "info", "selected history")
        .unwrap();
    let latest = catalog
        .create_deployment(&app_id, "manual", None, None)
        .unwrap();
    catalog
        .add_deployment_log(&latest, "info", "latest history")
        .unwrap();
    drop(catalog);
    let (cookies, _) = sign_in(&router).await;

    let response = get(
        &router,
        &format!("/apps/{app_id}/logs?deployment={selected}"),
        Some(&cookies),
    )
    .await;
    let body = body_text(response, 128 * 1024).await;
    assert!(body.contains("selected history"));
    assert!(!body.contains("latest history"));

    let response = get(
        &router,
        &format!("/apps/{app_id}/logs?deployment={other}"),
        Some(&cookies),
    )
    .await;
    let body = body_text(response, 128 * 1024).await;
    assert!(body.contains("latest history"));
    assert!(!body.contains("selected history"));
}
