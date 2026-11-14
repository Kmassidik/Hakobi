use axum::{
    body::{to_bytes, Body},
    http::{header::CONTENT_TYPE, Request, StatusCode},
};
use tower::ServiceExt;

use super::tests_support::{app, body_text, sign_in};

#[tokio::test]
async fn embedded_assets_are_served_with_their_content_type() {
    let (_data, app) = app();
    let response = app
        .oneshot(Request::get("/assets/app.css").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "text/css");
    let body = to_bytes(response.into_body(), 256 * 1024).await.unwrap();
    assert!(!body.is_empty());
}

#[tokio::test]
async fn overview_renders_readiness_and_dialog_and_serves_the_script() {
    let (_data, app) = app();
    let (authenticated, _csrf) = sign_in(&app).await;

    let dashboard = app
        .clone()
        .oneshot(
            Request::get("/dashboard")
                .header(axum::http::header::COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dashboard.status(), StatusCode::OK);
    let body = body_text(dashboard, 256 * 1024).await;
    assert!(body.contains("Compute"));
    assert!(body.contains("Runtime activity"));
    assert!(body.contains("Active containers"));
    assert!(body.contains("hakobi.js?v="));

    let script = app
        .oneshot(
            Request::get("/assets/hakobi.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(script.status(), StatusCode::OK);
    assert_eq!(
        script.headers().get(CONTENT_TYPE).unwrap(),
        "text/javascript"
    );
    let body = body_text(script, 64 * 1024).await;
    assert!(body.contains("delete-container"));
    assert!(body.contains("data-confirm-input"));
}
