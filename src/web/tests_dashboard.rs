use axum::{
    body::{to_bytes, Body},
    http::{
        header::{CONTENT_TYPE, COOKIE, ORIGIN, SET_COOKIE},
        Request, StatusCode,
    },
    Router,
};
use tower::ServiceExt;

use super::tests_support::{app, cookie};

#[tokio::test]
async fn root_redirects_to_dashboard() {
    let (_data, app) = app();
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers().get("location").unwrap(), "/dashboard");
}

async fn log_in(app: &Router) -> (String, String) {
    let login_page = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(login_page.status(), StatusCode::OK);
    let csrf = cookie(&login_page, "hakobi_csrf");
    let form = format!(
        "username=admin&password=correct+horse+battery+staple&csrf_token={}",
        csrf.split_once('=').unwrap().1
    );
    let login = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, csrf)
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let session = cookie(&login, "hakobi_session");
    let authenticated_csrf = cookie(&login, "hakobi_csrf");
    (session, authenticated_csrf)
}

async fn check_authenticated_dashboard(app: &Router, session: &str) {
    let dashboard = app
        .clone()
        .oneshot(
            Request::get("/dashboard")
                .header(COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dashboard.status(), StatusCode::OK);
    let body = to_bytes(dashboard.into_body(), 64 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("Runtime activity"));
    assert!(body.contains("aria-label=\"Mobile primary\""));
    assert!(body.contains("href=\"/apps\""));
    assert!(body.contains("href=\"/settings\""));
}

async fn check_status_authentication(app: &Router, session: &str) {
    let status = app
        .clone()
        .oneshot(
            Request::get("/partials/status")
                .header(COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let body = to_bytes(status.into_body(), 16 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Control plane online"));

    let unauthorized_status = app
        .clone()
        .oneshot(
            Request::get("/partials/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized_status.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        unauthorized_status.headers().get("hx-redirect").unwrap(),
        "/login"
    );
}

async fn reject_logout_with_invalid_csrf(app: &Router, session: &str) {
    let logout = app
        .clone()
        .oneshot(
            Request::post("/logout")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(COOKIE, session)
                .body(Body::from("csrf_token=invalid"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::FORBIDDEN);

    let still_authenticated = app
        .clone()
        .oneshot(
            Request::get("/dashboard")
                .header(COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(still_authenticated.status(), StatusCode::OK);
}

async fn complete_logout(app: Router, session: String, authenticated_csrf: String) {
    let logout = app
        .clone()
        .oneshot(
            Request::post("/logout")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(COOKIE, format!("{session}; {authenticated_csrf}"))
                .body(Body::from(format!(
                    "csrf_token={}",
                    authenticated_csrf.split_once('=').unwrap().1
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::SEE_OTHER);

    let after_logout = app
        .oneshot(
            Request::get("/dashboard")
                .header(COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after_logout.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn login_dashboard_and_logout_enforce_the_session() {
    let (_data, app) = app();
    let (session, authenticated_csrf) = log_in(&app).await;
    check_authenticated_dashboard(&app, &session).await;
    check_status_authentication(&app, &session).await;
    reject_logout_with_invalid_csrf(&app, &session).await;
    complete_logout(app, session, authenticated_csrf).await;
}

#[tokio::test]
async fn changing_usernames_does_not_bypass_client_throttling() {
    let (_data, app) = app();
    let login_page = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let mut csrf = cookie(&login_page, "hakobi_csrf");

    for attempt in 0..9 {
        let token = csrf.split_once('=').unwrap().1.to_owned();
        let form = format!("username=user{attempt}&password=wrong-password&csrf_token={token}");
        let response = app
            .clone()
            .oneshot(
                Request::post("/login")
                    .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(COOKIE, csrf)
                    .body(Body::from(form))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if attempt < 8 {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
        csrf = cookie(&response, "hakobi_csrf");
    }
}

#[tokio::test]
async fn login_requires_the_double_submit_csrf_token() {
    let (_data, app) = app();
    let response = app
        .oneshot(
            Request::post("/login")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    "username=admin&password=correct+horse+battery+staple&csrf_token=invalid",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.headers().get(SET_COOKIE).is_some());
    assert!(response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .all(|value| !value.to_str().unwrap().starts_with("hakobi_session=")));
}
