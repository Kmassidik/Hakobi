use axum::{
    body::Body,
    http::{
        header::{CONTENT_TYPE, COOKIE, ORIGIN},
        Request, StatusCode,
    },
};
use tower::ServiceExt;

use super::tests_support::{app, sign_in};

#[tokio::test]
async fn host_delete_unknown_id_returns_not_found() {
    let (_data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;

    let delete = app
        .clone()
        .oneshot(
            Request::post("/hosts/nonexistent-id/delete")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated)
                .body(Body::from(format!("csrf_token={csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn empty_or_long_fields_are_rejected() {
    let (_data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;

    // Empty name on app create.
    let empty_name = app
        .clone()
        .oneshot(
            Request::post("/apps")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!(
                    "name=&host_id=nope&build_type=dockerfile&csrf_token={csrf}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(empty_name.status(), StatusCode::BAD_REQUEST);

    // Name too long (65 chars).
    let long_name = "a".repeat(65);
    let long = app
        .clone()
        .oneshot(
            Request::post("/apps")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated)
                .body(Body::from(format!(
                    "name={long_name}&host_id=nope&build_type=dockerfile&csrf_token={csrf}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(long.status(), StatusCode::BAD_REQUEST);
}
