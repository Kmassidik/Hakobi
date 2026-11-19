use axum::{
    body::Body,
    http::{header::COOKIE, Request, StatusCode},
    Router,
};
use hakobi::catalog::Catalog;
use tower::ServiceExt;

use super::tests_support::{app, body_text, form_encode, post_form, sign_in};

fn runtime_payload(host_id: &str, csrf: &str) -> String {
    let fields = [
        ("name", "runtime-app"),
        ("host_id", host_id),
        ("build_type", "dockerfile"),
        ("github_repo", "owner/repo"),
        ("github_branch", "main"),
        ("start_command", "npm start"),
        ("release_command", "npm run migrate"),
        (
            "processes_json",
            r#"[{"name":"worker","command":"npm run worker"}]"#,
        ),
        (
            "volumes_json",
            r#"[{"name":"data","mount_path":"/var/lib/app","read_only":false}]"#,
        ),
        ("secrets_json", r#"{"DATABASE_URL":"postgres://private"}"#),
        ("csrf_token", csrf),
    ];
    fields
        .into_iter()
        .map(|(key, value)| format!("{key}={}", form_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

async fn assert_edit_form(app: &Router, authenticated: &str, app_id: &str) {
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/edit"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_text(response, 128 * 1024).await;
    assert!(body.contains("DATABASE_URL"));
    assert!(body.contains("npm run worker"));
    assert!(body.contains("/var/lib/app"));
    assert!(!body.contains("postgres://private"));
    assert!(body.contains("data-secret-value"));
}

#[tokio::test]
async fn runtime_options_persist_and_secrets_stay_write_only() {
    let (data, app) = app();
    let path = data.path().join("catalog.sqlite3");
    let host_id = Catalog::open(&path)
        .unwrap()
        .create_host_agent("runtime-host", "https://runtime.example.com", "tok_runtime")
        .unwrap();
    let (authenticated, csrf) = sign_in(&app).await;
    let response = post_form(
        &app,
        "/apps",
        &authenticated,
        "null",
        runtime_payload(&host_id, &csrf),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);

    let catalog = Catalog::open(&path).unwrap();
    let app_record = catalog.list_apps().unwrap().remove(0);
    assert_eq!(app_record.start_command.as_deref(), Some("npm start"));
    assert_eq!(
        app_record.release_command.as_deref(),
        Some("npm run migrate")
    );
    assert_eq!(
        catalog.list_app_processes(&app_record.id).unwrap()[0].name,
        "worker"
    );
    assert_eq!(
        catalog.list_app_volumes(&app_record.id).unwrap()[0].mount_path,
        "/var/lib/app"
    );
    assert_eq!(
        catalog
            .app_environment(&app_record)
            .unwrap()
            .get("DATABASE_URL")
            .map(String::as_str),
        Some("postgres://private")
    );

    assert_edit_form(&app, &authenticated, &app_record.id).await;
}

#[tokio::test]
async fn unsafe_mounts_and_duplicate_secret_names_are_rejected() {
    let (data, app) = app();
    let path = data.path().join("catalog.sqlite3");
    let host_id = Catalog::open(&path)
        .unwrap()
        .create_host_agent("runtime-host", "https://runtime.example.com", "tok_runtime")
        .unwrap();
    let (authenticated, csrf) = sign_in(&app).await;
    let payload = runtime_payload(&host_id, &csrf).replace("%2Fvar%2Flib%2Fapp", "..%2Fhost");
    let response = post_form(&app, "/apps", &authenticated, "null", payload).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(Catalog::open(&path)
        .unwrap()
        .list_apps()
        .unwrap()
        .is_empty());

    let payload = runtime_payload(&host_id, &csrf)
        + "&env_json="
        + &form_encode(r#"{"DATABASE_URL":"plain"}"#);
    let response = post_form(&app, "/apps", &authenticated, "null", payload).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(Catalog::open(&path)
        .unwrap()
        .list_apps()
        .unwrap()
        .is_empty());
}
