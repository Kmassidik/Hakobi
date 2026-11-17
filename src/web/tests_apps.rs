use std::path::Path;

use axum::{
    body::Body,
    http::{header::COOKIE, Request, StatusCode},
    Router,
};
use hakobi::catalog::Catalog;
use tower::ServiceExt;

use super::tests_support::{app, body_text, form_encode, post_form, sign_in};

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
    let (status, body) = get_page(app, authenticated, "/apps").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Create your first app"));
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
    reject_invalid_resources(app, authenticated, csrf, host_id).await;
}

async fn reject_invalid_resources(app: &Router, authenticated: &str, csrf: &str, host_id: &str) {
    let response = post_form(
        app,
        "/apps",
        authenticated,
        "null",
        format!(
            "name=web&host_id={host_id}&build_type=dockerfile&cpu_limit=0&memory_limit_mb=1&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
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
            "name=web&host_id={host_id}&build_type=dockerfile&github_repo={}&github_branch=main&github_dockerfile=Dockerfile&container_port=3000&cpu_limit=1.5&memory_limit_mb=512&csrf_token={csrf}",
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
        .contains("Build+started"));

    let catalog = Catalog::open(path).unwrap();
    let apps = catalog.list_apps().unwrap();
    assert_eq!(apps.len(), 1);
    assert!(apps[0].subdomain.is_none());
    assert_eq!(catalog.list_deployments(&apps[0].id).unwrap().len(), 1);
    assert_eq!(apps[0].cpu_limit, Some(1.5));
    assert_eq!(apps[0].memory_limit_bytes, Some(512 * 1024 * 1024));
    apps[0].id.clone()
}

async fn check_created_app_list(app: &Router, authenticated: &str, app_id: &str) {
    let (status, body) = get_page(app, authenticated, "/apps").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("web"));
    assert!(body.contains("dockerfile"));
    assert!(body.contains(&format!("/apps/{app_id}")));
}

async fn check_app_detail(app: &Router, authenticated: &str, app_id: &str) {
    let (status, body) = get_page(app, authenticated, &format!("/apps/{app_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Deployment history"));
    assert!(body.contains(&format!("/apps/{app_id}/deploy")));
    assert!(body.contains(&format!("/apps/{app_id}/edit")));
    assert!(body.contains(&format!("/apps/{app_id}/delete")));
    assert!(body.contains("delete-app"));
    assert!(body.contains("data-confirm-input"));
    assert!(body.contains("name=\"delete_data\""));
    assert!(body.contains("Permanently delete app-owned volumes and all data"));
    assert!(body.contains("App-owned networks are always deleted"));
    assert!(body.contains("Runtime"));
    assert!(body.contains("/runtime/start"));
    assert!(body.contains("data-app-usage"));
    assert!(body.contains("CPU usage (%)"));
    assert!(body.contains("Memory usage (MiB)"));
}

async fn check_edit_form(app: &Router, authenticated: &str, app_id: &str) {
    let (status, body) = get_page(app, authenticated, &format!("/apps/{app_id}/edit")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("value=\"web\""));
    assert!(body.contains("name=\"cpu_limit\""));
    assert!(body.contains("value=\"1.5\""));
    assert!(body.contains("name=\"memory_limit_mb\""));
    assert!(body.contains("value=\"512\""));
    assert!(body.contains("Edit app"));
    assert!(body.contains("data-app-form"));
    assert!(!body.contains("data-app-stepper"));
    assert!(!body.contains("data-step-progress"));
    assert!(!body.contains("data-step-navigation"));
    assert!(!body.contains("data-summary="));
    for step in 1..=4 {
        assert!(body.contains(&format!("data-app-step=\"{step}\"")));
    }
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
    assert!(updated.cpu_limit.is_none());
    assert!(updated.memory_limit_bytes.is_none());
}

async fn exercise_runtime_actions(
    app: &Router,
    path: &Path,
    authenticated: &str,
    csrf: &str,
    app_id: &str,
) {
    for (action, expected) in [
        ("start", "running"),
        ("pause", "paused"),
        ("unpause", "running"),
        ("restart", "running"),
        ("stop", "stopped"),
    ] {
        let response = post_form(
            app,
            &format!("/apps/{app_id}/runtime/{action}"),
            authenticated,
            "null",
            format!("csrf_token={csrf}"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let catalog = Catalog::open(path).unwrap();
        let app = catalog.get_app(app_id).unwrap().unwrap();
        assert_eq!(app.display_status(), expected);
    }
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
    let uri = format!("/apps/{app_id}/deployments");
    let (status, body) = get_page(app, authenticated, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("No deployments"));

    let uri = format!("/apps/{app_id}/logs");
    let (status, body) = get_page(app, authenticated, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("No log lines"));
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
    let wrong = post_form(
        app,
        &format!("/apps/{app_id}/delete"),
        authenticated,
        "null",
        format!("confirm_name=wrong&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    let delete = post_form(
        app,
        &format!("/apps/{app_id}/delete"),
        authenticated,
        "null",
        format!("confirm_name=web-2&csrf_token={csrf}"),
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
    reject_invalid_apps(&app, &authenticated, &csrf, &host_id).await;
    let app_id = create_app(&app, &path, &authenticated, &csrf, &host_id).await;
    check_created_app_list(&app, &authenticated, &app_id).await;
    check_app_detail(&app, &authenticated, &app_id).await;
    check_edit_form(&app, &authenticated, &app_id).await;
    update_app(&app, &path, &authenticated, &csrf, &host_id, &app_id).await;
    exercise_runtime_actions(&app, &path, &authenticated, &csrf, &app_id).await;
    deploy_and_rollback(&app, &authenticated, &csrf, &app_id).await;
    check_deployment_pages(&app, &authenticated, &app_id).await;
    reject_missing_and_forbidden_app(&app, &authenticated, &host_id).await;
    delete_app(&app, &path, &authenticated, &csrf, &app_id).await;
}

fn create_catalog_app(catalog: &mut Catalog, host_id: &str, name: &str) -> String {
    catalog
        .create_app(
            name,
            host_id,
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
        .unwrap()
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
    let app_id = create_catalog_app(&mut catalog, &host_id, "partial-env");
    (host_id, app_id)
}

async fn check_initial_status_partial(app: &Router, authenticated: &str, app_id: &str) {
    let (status, body) = get_page(app, authenticated, &format!("/apps/{app_id}/status")).await;
    assert_eq!(status, StatusCode::OK);
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
    let (_, body) = get_page(app, authenticated, &format!("/apps/{app_id}/status")).await;
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

#[tokio::test]
async fn app_and_deployment_lists_paginate() {
    let (data, app) = app();
    let (authenticated, _) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let (host_id, app_id) = create_partial_app(&path);
    let mut catalog = Catalog::open(&path).unwrap();
    for number in 1..=20 {
        create_catalog_app(&mut catalog, &host_id, &format!("app-{number}"));
    }
    for _ in 0..12 {
        catalog
            .create_deployment(&app_id, "manual", None, None)
            .unwrap();
    }
    drop(catalog);

    let (_, first) = get_page(&app, &authenticated, "/apps?notice=Created&filter=live").await;
    assert!(first.contains("Showing 1–20 of 21 · Page 1 of 2"));
    assert!(first.contains("filter=live"));
    assert!(first.contains("page=2"));
    assert!(!first.contains("notice="));
    let (_, second) = get_page(&app, &authenticated, "/apps?page=99").await;
    assert!(second.contains("Showing 21–21 of 21 · Page 2 of 2"));
    assert!(second.contains("app-20"));

    let uri = format!("/apps/{app_id}?notice=Created&filter=live");
    let (_, detail) = get_page(&app, &authenticated, &uri).await;
    assert!(detail.contains("Showing 1–10 of 12 · Page 1 of 2"));
    assert!(detail.contains(">v12</td>"));
    assert!(!detail.contains(">v2</td>"));
    assert!(detail.contains("filter=live"));
    assert!(detail.contains("deployments_page=2"));
    assert!(!detail.contains("notice="));

    let uri = format!("/apps/{app_id}?deployments_page=99");
    let (_, detail) = get_page(&app, &authenticated, &uri).await;
    assert!(detail.contains("Showing 11–12 of 12 · Page 2 of 2"));
    assert!(detail.contains(">v2</td>"));
    assert!(detail.contains("Latest deployment: v12"));
    assert!(detail.contains("Rollback to v11"));
    let uri = format!("/apps/{app_id}/deployments?filter=live");
    let (_, standalone) = get_page(&app, &authenticated, &uri).await;
    assert!(standalone.contains(&format!("/apps/{app_id}/deployments?")));
    assert!(standalone.contains("filter=live"));
    assert!(standalone.contains("deployments_page=2"));
    let uri = format!("/apps/{app_id}/deployments?deployments_page=2");
    let (_, second) = get_page(&app, &authenticated, &uri).await;
    assert!(second.contains("Showing 11–12 of 12 · Page 2 of 2"));
    assert!(second.contains(">v2</td>"));
}
