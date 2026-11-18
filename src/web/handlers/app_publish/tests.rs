use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode},
    response::Redirect,
    routing::get,
    Router,
};
use hakobi::catalog::Catalog;
use tempfile::TempDir;

use super::{publication_health_result, verify_public_health, AppState};
use crate::web::tests_support::{body_text, config, FakeBackend};

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{address}/")
}

#[tokio::test]
async fn health_accepts_final_success_after_redirects() {
    let router = Router::new()
        .route("/", get(|| async { Redirect::temporary("/ready") }))
        .route("/ready", get(|| async { StatusCode::NO_CONTENT }));
    let url = serve(router).await;

    let health = verify_public_health(&url, Duration::ZERO).await;

    assert_eq!(health, Ok(()));
}

#[tokio::test]
async fn health_retries_until_success() {
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);
    let router = Router::new().route(
        "/",
        get(move || {
            let observed = Arc::clone(&observed);
            async move {
                if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::NO_CONTENT
                }
            }
        }),
    );
    let url = serve(router).await;

    let health = verify_public_health(&url, Duration::ZERO).await;

    assert_eq!(health, Ok(()));
    assert_eq!(requests.load(Ordering::SeqCst), 2);
}

fn publication_state() -> (TempDir, AppState, String) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).unwrap();
    let host_id = catalog
        .create_host_agent(
            "publish-host",
            "https://publish.example.com",
            "tok_publish_01",
        )
        .unwrap();
    let app_id = catalog
        .create_app(
            "demo",
            &host_id,
            "dockerfile",
            None,
            None,
            None,
            Some("Dockerfile"),
            false,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    catalog
        .set_app_runtime_status(&app_id, Some("running"))
        .unwrap();
    catalog.set_app_host_port(&app_id, Some(18080)).unwrap();
    catalog.set_app_subdomain(&app_id, Some("demo")).unwrap();
    catalog
        .settings_set(crate::cloudflare::apps_route::KEY_DOMAIN, "example.test")
        .unwrap();
    drop(catalog);
    let state = crate::web::build_app_state(config(), path, Arc::new(FakeBackend), None).unwrap();
    (directory, state, app_id)
}

#[tokio::test]
async fn failed_health_clears_assignment_without_success_notice() {
    let (_directory, state, app_id) = publication_state();
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);
    let router = Router::new().route(
        "/",
        get(move || {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                StatusCode::NOT_FOUND
            }
        }),
    );
    let url = serve(router).await;
    let error = verify_public_health(&url, Duration::ZERO)
        .await
        .unwrap_err();
    assert_eq!(error, "HTTP 404 Not Found");
    assert_eq!(requests.load(Ordering::SeqCst), 10);

    let mut headers = HeaderMap::new();
    headers.insert("hx-request", HeaderValue::from_static("true"));
    let response =
        publication_health_result(&state, &headers, &app_id, "demo.example.test", Err(error)).await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let catalog = Catalog::open(state.catalog_path.as_path()).unwrap();
    assert_eq!(catalog.get_app(&app_id).unwrap().unwrap().subdomain, None);
    let body = body_text(response, 128 * 1024).await;
    assert!(body.contains("demo.example.test"));
    assert!(body.contains("HTTP 404 Not Found"));
    assert!(body.contains("managed route remains configured"));
    assert!(body.contains("Publish application"));
    assert!(!body.contains("Application is alive"));
    assert!(!body.contains("Public application"));
}
