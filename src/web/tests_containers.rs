use axum::{
    body::Body,
    http::{header::COOKIE, Request, StatusCode},
    Router,
};
use hakobi::catalog::Catalog;
use tempfile::TempDir;
use tower::ServiceExt;

use super::tests_support::{app, body_text, post_form, sign_in};

fn create_provider(data: &TempDir) -> String {
    let path = data.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).unwrap();
    catalog
        .create_provider_connection("local", "docker", "unix:///var/run/docker.sock", "")
        .unwrap()
}

async fn check_creation_form(app: &Router, authenticated: &str, csrf: &str, provider: &str) {
    let anonymous = app
        .clone()
        .oneshot(Request::get("/containers/new").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);

    let form_page = app
        .clone()
        .oneshot(
            Request::get("/containers/new")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(form_page.status(), StatusCode::OK);
    let body = body_text(form_page, 128 * 1024).await;
    assert!(body.contains("Run container"));
    assert!(body.contains("Container port"));
    assert!(body.contains("name=\"provider_id\""));
    assert!(body.contains("local"));
    assert!(body.contains(provider));
    assert!(body.contains(&format!("value=\"{csrf}\"")));
}

async fn reject_creation_without_valid_auth(
    app: &Router,
    authenticated: &str,
    csrf: &str,
    provider: &str,
    create: &str,
) {
    let no_csrf = post_form(
        app,
        "/containers/new",
        authenticated,
        "null",
        format!("provider_id={provider}&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token=invalid"),
    )
    .await;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);

    let bad_origin = post_form(
        app,
        "/containers/new",
        authenticated,
        "http://attacker.example",
        format!("{create}{csrf}"),
    )
    .await;
    assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

    let missing_provider = post_form(
        app,
        "/containers/new",
        authenticated,
        "null",
        format!("provider_id=missing&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(missing_provider.status(), StatusCode::NOT_FOUND);
}

async fn reject_invalid_image_and_name(
    app: &Router,
    authenticated: &str,
    csrf: &str,
    provider: &str,
) {
    let bad_image = post_form(
        app,
        "/containers/new",
        authenticated,
        "null",
        format!("provider_id={provider}&image=bad+image&name=demo-api&container_port=8080&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_image.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(bad_image, 128 * 1024)
        .await
        .contains("whitespace"));

    let bad_name = post_form(
        app,
        "/containers/new",
        authenticated,
        "null",
        format!("provider_id={provider}&image=nginx%3Alatest&name=bad%2Fname&container_port=8080&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_name.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(bad_name, 128 * 1024).await.contains("letters"));
}

async fn reject_invalid_port(app: &Router, authenticated: &str, csrf: &str, provider: &str) {
    let bad_port = post_form(
        app,
        "/containers/new",
        authenticated,
        "null",
        format!("provider_id={provider}&image=nginx%3Alatest&name=demo-api&container_port=0&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_port.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(bad_port, 128 * 1024)
        .await
        .contains("Container port must be"));
}

async fn create_container(app: &Router, authenticated: &str, csrf: &str, create: &str) -> String {
    let created = post_form(
        app,
        "/containers/new",
        authenticated,
        "null",
        format!("{create}{csrf}"),
    )
    .await;
    assert_eq!(created.status(), StatusCode::SEE_OTHER);
    let location = created
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(location.starts_with("/containers?notice="));
    assert!(location.contains("18050"));
    assert!(location.contains("demo-api"));
    location
}

async fn check_container_list(app: &Router, authenticated: &str, provider: &str, location: &str) {
    let listed = app
        .clone()
        .oneshot(
            Request::get(location)
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let body = body_text(listed, 256 * 1024).await;
    assert!(body.contains("is running on host port 18050."));
    assert!(body.contains(&format!("/containers/{provider}/demo-api/start")));
    assert!(body.contains(&format!("/containers/{provider}/demo-api/stop")));
    assert!(body.contains(&format!("/containers/{provider}/demo-api/restart")));
    assert!(body.contains(&format!(
        "data-delete-action=\"/containers/{provider}/demo-api/delete\""
    )));
    assert!(body.contains("data-delete-name=\"demo-api\""));
    assert!(body.contains("data-dialog-open=\"delete-container\""));
    assert!(body.contains("id=\"delete-container\""));
    assert!(body.contains("data-confirm-input"));
    assert!(body.contains("href=\"/containers/new\""));
}

async fn reject_action_without_valid_auth(
    app: &Router,
    authenticated: &str,
    csrf: &str,
    provider: &str,
    action: &str,
) {
    let forbidden = post_form(
        app,
        &format!("/containers/{provider}/demo-api/{action}"),
        authenticated,
        "null",
        "csrf_token=invalid".to_owned(),
    )
    .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let bad_origin = post_form(
        app,
        &format!("/containers/{provider}/demo-api/{action}"),
        authenticated,
        "http://attacker.example",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

    let unknown = post_form(
        app,
        &format!("/containers/missing/demo-api/{action}"),
        authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
}

async fn check_action(
    app: &Router,
    authenticated: &str,
    csrf: &str,
    provider: &str,
    action: &str,
    word: &str,
) {
    reject_action_without_valid_auth(app, authenticated, csrf, provider, action).await;
    let invalid_name = post_form(
        app,
        &format!("/containers/{provider}/bad%2Fname/{action}"),
        authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(invalid_name.status(), StatusCode::BAD_REQUEST);

    let ok = post_form(
        app,
        &format!("/containers/{provider}/demo-api/{action}"),
        authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::SEE_OTHER);
    let location = ok.headers().get("location").unwrap().to_str().unwrap();
    assert!(location.contains(&format!("demo-api+{word}")));
}

async fn reject_invalid_deletes(app: &Router, authenticated: &str, csrf: &str, provider: &str) {
    let delete_without_csrf = post_form(
        app,
        &format!("/containers/{provider}/demo-api/delete"),
        authenticated,
        "null",
        "confirm_name=demo-api&csrf_token=invalid".to_owned(),
    )
    .await;
    assert_eq!(delete_without_csrf.status(), StatusCode::FORBIDDEN);

    let delete_wrong_name = post_form(
        app,
        &format!("/containers/{provider}/demo-api/delete"),
        authenticated,
        "null",
        format!("confirm_name=other&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(delete_wrong_name.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(delete_wrong_name, 64 * 1024)
        .await
        .contains("does not match"));

    let delete_unknown_provider = post_form(
        app,
        "/containers/missing/demo-api/delete",
        authenticated,
        "null",
        format!("confirm_name=demo-api&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(delete_unknown_provider.status(), StatusCode::NOT_FOUND);
}

async fn delete_container(app: &Router, authenticated: &str, csrf: &str, provider: &str) {
    let deleted = post_form(
        app,
        &format!("/containers/{provider}/demo-api/delete"),
        authenticated,
        "null",
        format!("confirm_name=demo-api&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::SEE_OTHER);
    assert!(deleted
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("removed"));
}

async fn check_disabled_provider(
    app: Router,
    data: &TempDir,
    authenticated: &str,
    csrf: &str,
    provider: &str,
    create: &str,
) {
    let path = data.path().join("catalog.sqlite3");
    {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog.set_provider_enabled(provider, false).unwrap();
    }
    let disabled = post_form(
        &app,
        "/containers/new",
        authenticated,
        "null",
        format!("{create}{csrf}"),
    )
    .await;
    assert_eq!(disabled.status(), StatusCode::NOT_FOUND);

    let form_page = app
        .oneshot(
            Request::get("/containers/new")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(form_page.status(), StatusCode::OK);
    assert!(body_text(form_page, 128 * 1024)
        .await
        .contains("No enabled Docker providers"));
}

#[tokio::test]
async fn container_creation_and_lifecycle_actions_are_guarded() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let provider = create_provider(&data);
    let create = format!(
        "provider_id={provider}&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token="
    );
    check_creation_form(&app, &authenticated, &csrf, &provider).await;
    reject_creation_without_valid_auth(&app, &authenticated, &csrf, &provider, &create).await;
    reject_invalid_image_and_name(&app, &authenticated, &csrf, &provider).await;
    reject_invalid_port(&app, &authenticated, &csrf, &provider).await;
    let location = create_container(&app, &authenticated, &csrf, &create).await;
    check_container_list(&app, &authenticated, &provider, &location).await;
    for (action, word) in [
        ("start", "started"),
        ("stop", "stopped"),
        ("restart", "restarted"),
    ] {
        check_action(&app, &authenticated, &csrf, &provider, action, word).await;
    }
    reject_invalid_deletes(&app, &authenticated, &csrf, &provider).await;
    delete_container(&app, &authenticated, &csrf, &provider).await;
    check_disabled_provider(app, &data, &authenticated, &csrf, &provider, &create).await;
}
