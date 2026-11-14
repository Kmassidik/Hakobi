use std::path::Path;

use axum::{
    body::Body,
    http::{header::COOKIE, Request, StatusCode},
    Router,
};
use hakobi::catalog::Catalog;
use tower::ServiceExt;

use super::tests_support::{app, body_text, form_encode, post_form, sign_in};

fn create_host(path: &Path) -> String {
    let mut catalog = Catalog::open(path).unwrap();
    catalog
        .create_host_agent(
            "env-http-host",
            "https://env-http.example.com",
            "tok_env_http_01",
        )
        .unwrap()
}

async fn check_empty_app_list(app: &Router, authenticated: &str) {
    let list = app
        .clone()
        .oneshot(
            Request::get("/apps")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let body = body_text(list, 128 * 1024).await;
    assert!(body.contains("Create your first app"));
}

async fn check_new_app_form(app: &Router, authenticated: &str) {
    let new_page = app
        .clone()
        .oneshot(
            Request::get("/apps/new")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(new_page.status(), StatusCode::OK);
    let body = body_text(new_page, 128 * 1024).await;
    assert!(body.contains("New app"));
    assert!(body.contains("name=\"host_id\""));
    assert!(body.contains("name=\"csrf_token\""));
    assert!(body.contains("name=\"github_repo\""));
    assert!(body.contains("env-http-host"));
}

async fn reject_invalid_apps(app: &Router, authenticated: &str, csrf: &str, host_id: &str) {
    let invalid = post_form(
        app,
        "/apps",
        authenticated,
        "null",
        format!(
            "name=web&host_id={host_id}&build_type=helm&github_repo=example%2Frepo&github_branch=main&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let invalid = post_form(
        app,
        "/apps",
        authenticated,
        "null",
        format!(
            "name=web&host_id={host_id}&build_type=compose&github_repo=&github_branch=main&container_port=0&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let invalid_host = post_form(
        app,
        "/apps",
        authenticated,
        "null",
        format!(
            "name=web&host_id=no-such-host&build_type=compose&github_repo=&github_branch=main&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(invalid_host.status(), StatusCode::BAD_REQUEST);
}

async fn create_app(
    app: &Router,
    path: &Path,
    authenticated: &str,
    csrf: &str,
    host_id: &str,
) -> String {
    let create = post_form(
        app,
        "/apps",
        authenticated,
        "null",
        format!(
            "name=web&host_id={host_id}&build_type=dockerfile&github_repo={}&github_branch=main&github_dockerfile=Dockerfile&container_port=3000&csrf_token={csrf}",
            form_encode("user/repo")
        ),
    )
    .await;
    assert_eq!(create.status(), StatusCode::SEE_OTHER);
    assert!(create
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("App+created"));

    let catalog = Catalog::open(path).unwrap();
    let apps = catalog.list_apps().unwrap();
    assert_eq!(apps.len(), 1);
    apps[0].id.clone()
}

async fn check_created_app_list(app: &Router, authenticated: &str, app_id: &str) {
    let list = app
        .clone()
        .oneshot(
            Request::get("/apps")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let body = body_text(list, 128 * 1024).await;
    assert!(body.contains("web"));
    assert!(body.contains("dockerfile"));
    assert!(body.contains(&format!("/apps/{app_id}")));
}

async fn check_app_detail(app: &Router, authenticated: &str, app_id: &str) {
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
    let body = body_text(detail, 128 * 1024).await;
    assert!(body.contains("Deployment history"));
    assert!(body.contains(&format!("/apps/{app_id}/deploy")));
    assert!(body.contains(&format!("/apps/{app_id}/edit")));
    assert!(body.contains(&format!("/apps/{app_id}/delete")));
    assert!(body.contains("delete-app"));
    assert!(body.contains("data-confirm-input"));
}

async fn check_edit_form(app: &Router, authenticated: &str, app_id: &str) {
    let edit_page = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/edit"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(edit_page.status(), StatusCode::OK);
    let body = body_text(edit_page, 128 * 1024).await;
    assert!(body.contains("value=\"web\""));
    assert!(body.contains("Edit app"));
}

async fn update_app(
    app: &Router,
    path: &Path,
    authenticated: &str,
    csrf: &str,
    host_id: &str,
    app_id: &str,
) {
    let update = post_form(
        app,
        &format!("/apps/{app_id}/edit"),
        authenticated,
        "null",
        format!(
            "name=web-2&host_id={host_id}&build_type=dockerfile&github_repo=&github_branch=&github_dockerfile=Dockerfile&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(update.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(path).unwrap();
    let updated = catalog.get_app(app_id).unwrap().unwrap();
    assert_eq!(updated.name, "web-2");
    assert_eq!(updated.build_type, "dockerfile");
    assert!(updated.github_repo.is_none());
    assert!(updated.github_branch.is_none());
    assert_eq!(updated.github_dockerfile.as_deref(), Some("Dockerfile"));
    assert!(!updated.github_auto_deploy);
    assert!(updated.container_port.is_none());
}

async fn deploy_and_rollback(app: &Router, authenticated: &str, csrf: &str, app_id: &str) {
    let deploy = post_form(
        app,
        &format!("/apps/{app_id}/deploy"),
        authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(deploy.status(), StatusCode::SEE_OTHER);

    let rollback = post_form(
        app,
        &format!("/apps/{app_id}/rollback"),
        authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(rollback.status(), StatusCode::SEE_OTHER);
}

async fn check_deployment_pages(app: &Router, authenticated: &str, app_id: &str) {
    let deployments = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/deployments"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deployments.status(), StatusCode::OK);
    assert!(!body_text(deployments, 128 * 1024)
        .await
        .contains("No deployments"));

    let logs = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/logs"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs.status(), StatusCode::OK);
    assert!(!body_text(logs, 128 * 1024).await.contains("No log lines"));
}

async fn reject_missing_and_forbidden_app(app: &Router, authenticated: &str, host_id: &str) {
    let missing = app
        .clone()
        .oneshot(
            Request::get("/apps/nope")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let forbidden = post_form(
        app,
        "/apps",
        authenticated,
        "https://evil.example.com",
        format!(
            "name=evil&host_id={host_id}&build_type=compose&github_repo=&github_branch=&csrf_token=wrong"
        ),
    )
    .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
}

async fn delete_app(app: &Router, path: &Path, authenticated: &str, csrf: &str, app_id: &str) {
    let delete = post_form(
        app,
        &format!("/apps/{app_id}/delete"),
        authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(delete.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(path).unwrap();
    assert!(catalog.list_apps().unwrap().is_empty());
}

#[tokio::test]
async fn apps_can_be_created_edited_and_deleted_over_http() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let host_id = create_host(&path);
    check_empty_app_list(&app, &authenticated).await;
    check_new_app_form(&app, &authenticated).await;
    reject_invalid_apps(&app, &authenticated, &csrf, &host_id).await;
    let app_id = create_app(&app, &path, &authenticated, &csrf, &host_id).await;
    check_created_app_list(&app, &authenticated, &app_id).await;
    check_app_detail(&app, &authenticated, &app_id).await;
    check_edit_form(&app, &authenticated, &app_id).await;
    update_app(&app, &path, &authenticated, &csrf, &host_id, &app_id).await;
    deploy_and_rollback(&app, &authenticated, &csrf, &app_id).await;
    check_deployment_pages(&app, &authenticated, &app_id).await;
    reject_missing_and_forbidden_app(&app, &authenticated, &host_id).await;
    delete_app(&app, &path, &authenticated, &csrf, &app_id).await;
}

fn create_partial_app(path: &Path) -> (String, String) {
    let mut catalog = Catalog::open(path).unwrap();
    let host_id = catalog
        .create_host_agent(
            "partial-host",
            "https://partial.example.com",
            "tok_partial_01",
        )
        .unwrap();
    let app_id = catalog
        .create_app(
            "partial-env",
            &host_id,
            "dockerfile",
            None,
            None,
            None,
            Some("Dockerfile"),
            false,
            None,
        )
        .unwrap();
    (host_id, app_id)
}

async fn check_initial_status_partial(app: &Router, authenticated: &str, app_id: &str) {
    let partial = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/status"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(partial.status(), StatusCode::OK);
    let body = body_text(partial, 64 * 1024).await;
    assert!(body.contains("id=\"app-status\""));
    assert!(body.contains("every 5s"));
    assert!(body.contains("idle"));
    assert!(body.contains("No deployments yet."));
}

fn create_partial_deployment(path: &Path, app_id: &str) {
    let mut catalog = Catalog::open(path).unwrap();
    let deployment_id = catalog
        .create_deployment(app_id, "manual", None, None)
        .unwrap();
    catalog
        .update_deployment_status(&deployment_id, "running", None)
        .unwrap();
    catalog
        .add_deployment_log(&deployment_id, "info", "deployed v1")
        .unwrap();
}

async fn check_updated_status_partial(app: &Router, authenticated: &str, app_id: &str) {
    let partial = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/status"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_text(partial, 64 * 1024).await;
    assert!(body.contains("running"));
    assert!(body.contains("v1"));
}

#[tokio::test]
async fn app_status_partial_refreshes_with_htmx() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let (host_id, app_id) = create_partial_app(&path);
    check_initial_status_partial(&app, &authenticated, &app_id).await;
    create_partial_deployment(&path, &app_id);
    check_updated_status_partial(&app, &authenticated, &app_id).await;
    let _ = host_id;
    let _ = csrf;
}
