use std::sync::Arc;

use super::{auth, router_with_backend};
use crate::config::PublicConfig;
use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{
        header::{CONTENT_TYPE, COOKIE, ORIGIN, SET_COOKIE},
        Request, StatusCode,
    },
    response::Response,
    Router,
};
use hakobi::{
    catalog::Catalog,
    providers::{
        ContainerBackend, ContainerPort, ContainerRow, DockerSnapshot, HostInfo, ImageRow,
        NetworkRow, RunResult, VolumeRow,
    },
};
use tempfile::TempDir;
use tower::ServiceExt;

struct FakeBackend;

#[async_trait]
impl ContainerBackend for FakeBackend {
    async fn test(&self, _endpoint: &str) -> anyhow::Result<String> {
        Ok("Docker test · API test".into())
    }

    async fn containers(&self, _endpoint: &str) -> anyhow::Result<Vec<ContainerRow>> {
        Ok(vec![ContainerRow {
            id: "0123456789abcdef".into(),
            name: "demo-api".into(),
            image: "hakobi/demo:latest".into(),
            state: "running".into(),
            status: "Up 2 hours".into(),
            health: Some("healthy".into()),
            compose_project: Some("hakobi-stack".into()),
            compose_service: Some("api".into()),
            ports: vec![ContainerPort {
                host_ip: Some("0.0.0.0".into()),
                host_port: Some(18080),
                container_port: 8080,
                protocol: "tcp".into(),
            }],
        }])
    }

    async fn snapshot(&self, _endpoint: &str) -> anyhow::Result<DockerSnapshot> {
        Ok(DockerSnapshot {
            host: HostInfo {
                name: "fedora-test".into(),
                server_version: "28.0.1".into(),
                containers: 2,
                running: 1,
                images: 1,
                operating_system: "Fedora Linux 44".into(),
                architecture: "x86_64".into(),
                cpus: 16,
            },
            images: vec![ImageRow {
                id: "sha256:abcdef012345".into(),
                tags: vec!["alpine:3.20".into()],
                size_bytes: 8 * 1_048_576,
            }],
            volumes: vec![VolumeRow {
                name: "hakobi-data".into(),
                driver: "local".into(),
                mountpoint: "/var/lib/docker/volumes/hakobi-data/_data".into(),
            }],
            networks: vec![NetworkRow {
                name: "bridge".into(),
                driver: "bridge".into(),
                scope: "local".into(),
            }],
        })
    }

    async fn start_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn restart_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn run_container(
        &self,
        _endpoint: &str,
        _image: &str,
        name: &str,
        _container_port: u16,
    ) -> anyhow::Result<RunResult> {
        Ok(RunResult {
            container_id: "fedcba9876543210".into(),
            host_port: 18050,
            name: name.to_owned(),
        })
    }
}

fn app() -> (TempDir, Router) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.sqlite3");
    (
        directory,
        router_with_backend(config(), path, Arc::new(FakeBackend)),
    )
}

fn config() -> PublicConfig {
    PublicConfig {
        username: "admin".into(),
        password_hash: auth::hash_password("correct horse battery staple").unwrap(),
        base_url: "http://127.0.0.1:3000".into(),
        secure_cookies: false,
        tunnel_enabled: false,
    }
}

fn cookie(response: &Response, name: &str) -> String {
    response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with(&format!("{name}=")))
        .and_then(|value| value.split(';').next())
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn login_dashboard_and_logout_enforce_the_session() {
    let (_data, app) = app();
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

    let dashboard = app
        .clone()
        .oneshot(
            Request::get("/")
                .header(COOKIE, session.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dashboard.status(), StatusCode::OK);
    let body = to_bytes(dashboard.into_body(), 64 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Hello, world."));

    let status = app
        .clone()
        .oneshot(
            Request::get("/partials/status")
                .header(COOKIE, session.clone())
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

    let logout = app
        .clone()
        .oneshot(
            Request::post("/logout")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(COOKIE, session.clone())
                .body(Body::from("csrf_token=invalid"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::FORBIDDEN);

    let still_authenticated = app
        .clone()
        .oneshot(
            Request::get("/")
                .header(COOKIE, session.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(still_authenticated.status(), StatusCode::OK);

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
            Request::get("/")
                .header(COOKIE, session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(after_logout.status(), StatusCode::SEE_OTHER);
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

#[tokio::test]
async fn providers_and_containers_work_end_to_end() {
    let (data, app) = app();
    let login_page = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let csrf_before_login = cookie(&login_page, "hakobi_csrf");
    let login = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, csrf_before_login.clone())
                .body(Body::from(format!(
                    "username=admin&password=correct+horse+battery+staple&csrf_token={}",
                    csrf_before_login.split_once('=').unwrap().1
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let session = cookie(&login, "hakobi_session");
    let csrf = cookie(&login, "hakobi_csrf");
    let authenticated = format!("{session}; {csrf}");
    let csrf_value = csrf.split_once('=').unwrap().1;

    let providers = app
        .clone()
        .oneshot(
            Request::get("/providers")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(providers.status(), StatusCode::OK);
    let body = to_bytes(providers.into_body(), 64 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("Add Docker provider"));

    let add = app
        .clone()
        .oneshot(
            Request::post("/providers")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!(
                    "name=local&endpoint=unix%3A%2F%2F%2Fvar%2Frun%2Fdocker.sock&secret=top-secret&csrf_token={csrf_value}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(add.status(), StatusCode::SEE_OTHER);

    let path = data.path().join("catalog.sqlite3");
    let catalog = Catalog::open(&path).unwrap();
    let provider = catalog.list_provider_connections().unwrap().remove(0);
    assert_eq!(provider.name, "local");
    assert_eq!(provider.kind, "docker");
    assert!(provider.enabled);
    drop(catalog);

    let visible = app
        .clone()
        .oneshot(
            Request::get("/providers")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(visible.into_body(), 64 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("local"));
    assert!(!body.contains("top-secret"));

    let test = app
        .clone()
        .oneshot(
            Request::post(format!("/providers/{}/test", provider.id))
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!("csrf_token={csrf_value}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(test.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(&path).unwrap();
    let checked = catalog.list_provider_connections().unwrap().remove(0);
    assert!(checked.last_checked_at.is_some());
    assert!(checked.last_error.is_none());
    drop(catalog);

    let containers = app
        .clone()
        .oneshot(
            Request::get("/containers")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(containers.status(), StatusCode::OK);
    let body = to_bytes(containers.into_body(), 64 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("demo-api"));
    assert!(body.contains("hakobi/demo:latest"));
    assert!(body.contains("hakobi-stack"));
    assert!(body.contains("18080"));
    assert!(body.contains("healthy"));

    let infrastructure = app
        .clone()
        .oneshot(
            Request::get("/infrastructure")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(infrastructure.status(), StatusCode::OK);
    let body = to_bytes(infrastructure.into_body(), 128 * 1024)
        .await
        .unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("fedora-test"));
    assert!(body.contains("alpine:3.20"));
    assert!(body.contains("hakobi-data"));
    assert!(body.contains("bridge"));

    let stacks = app
        .clone()
        .oneshot(
            Request::get("/stacks")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stacks.status(), StatusCode::OK);
    let body = to_bytes(stacks.into_body(), 64 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("hakobi-stack"));
    assert!(body.contains("api"));
    assert!(body.contains("18000"));

    let stack_detail = app
        .clone()
        .oneshot(
            Request::get(format!("/stacks/{}/hakobi-stack", provider.id))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stack_detail.status(), StatusCode::OK);
    let body = to_bytes(stack_detail.into_body(), 64 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("demo-api"));
    assert!(body.contains("hakobi-stack"));

    let toggle = app
        .clone()
        .oneshot(
            Request::post(format!("/providers/{}/toggle", provider.id))
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!("csrf_token={csrf_value}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(toggle.status(), StatusCode::SEE_OTHER);
    let containers = app
        .clone()
        .oneshot(
            Request::get("/containers")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(containers.into_body(), 64 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("No enabled Docker providers"));

    let delete = app
        .oneshot(
            Request::post(format!("/providers/{}/delete", provider.id))
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated)
                .body(Body::from(format!("csrf_token={csrf_value}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(&path).unwrap();
    assert!(catalog.list_provider_connections().unwrap().is_empty());
}

fn form_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

async fn sign_in(app: &Router) -> (String, String) {
    let login_page = app
        .clone()
        .oneshot(Request::get("/login").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let csrf_before_login = cookie(&login_page, "hakobi_csrf");
    let login = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, csrf_before_login.clone())
                .body(Body::from(format!(
                    "username=admin&password=correct+horse+battery+staple&csrf_token={}",
                    csrf_before_login.split_once('=').unwrap().1
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SEE_OTHER);
    let session = cookie(&login, "hakobi_session");
    let csrf = cookie(&login, "hakobi_csrf");
    let csrf_value = csrf.split_once('=').unwrap().1.to_owned();
    (format!("{session}; {csrf}"), csrf_value)
}

#[tokio::test]
async fn managed_stack_definitions_can_be_created_edited_and_deleted() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;

    let path = data.path().join("catalog.sqlite3");
    let provider_id = {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog
            .create_provider_connection("local", "docker", "unix:///var/run/docker.sock", "")
            .unwrap()
    };

    let new_page = app
        .clone()
        .oneshot(
            Request::get("/stacks/new")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(new_page.status(), StatusCode::OK);
    let body = to_bytes(new_page.into_body(), 128 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("New stack"));
    assert!(body.contains("Definition YAML"));
    assert!(body.contains("local"));

    let invalid = app
        .clone()
        .oneshot(
            Request::post("/stacks/new")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!(
                    "kind=compose&provider_id={provider_id}&name=web&source=.&yaml=&csrf_token={csrf}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(invalid.into_body(), 128 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("empty"));

    let manifest = "services:\n  web:\n    image: nginx:latest";
    let create = app
        .clone()
        .oneshot(
            Request::post("/stacks/new")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!(
                    "kind=compose&provider_id={provider_id}&name=web&source=.&yaml={}&csrf_token={csrf}",
                    form_encode(manifest)
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::SEE_OTHER);
    assert!(create
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("Stack+created"));

    let stack = {
        let catalog = Catalog::open(&path).unwrap();
        catalog.list_stacks().unwrap().remove(0)
    };
    assert_eq!(stack.name, "web");
    assert_eq!(stack.kind.as_str(), "compose");

    let stacks = app
        .clone()
        .oneshot(
            Request::get("/stacks")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stacks.status(), StatusCode::OK);
    let body = to_bytes(stacks.into_body(), 128 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("Managed stacks"));
    assert!(body.contains("web"));
    assert!(body.contains(&format!("/stacks/definitions/{}/edit", stack.id)));
    assert!(body.contains(&format!("/stacks/definitions/{}/delete", stack.id)));
    assert!(body.contains("data-delete-name=\"web\""));
    assert!(body.contains("id=\"delete-stack\""));
    assert!(body.contains("data-confirm-input"));

    let edit_page = app
        .clone()
        .oneshot(
            Request::get(format!("/stacks/definitions/{}/edit", stack.id))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(edit_page.status(), StatusCode::OK);
    let body = to_bytes(edit_page.into_body(), 128 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("Edit stack"));
    assert!(body.contains("nginx:latest"));

    let update = app
        .clone()
        .oneshot(
            Request::post(format!("/stacks/definitions/{}/edit", stack.id))
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!(
                    "kind=compose&name=api&source=.&yaml={}&csrf_token={csrf}",
                    form_encode(manifest)
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update.status(), StatusCode::SEE_OTHER);
    {
        let catalog = Catalog::open(&path).unwrap();
        let updated = catalog.get_stack(&stack.id).unwrap().unwrap();
        assert_eq!(updated.name, "api");
    }

    let delete_page = app
        .clone()
        .oneshot(
            Request::get(format!("/stacks/definitions/{}/delete", stack.id))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_page.status(), StatusCode::OK);

    let wrong_name = app
        .clone()
        .oneshot(
            Request::post(format!("/stacks/definitions/{}/delete", stack.id))
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated.clone())
                .body(Body::from(format!("confirm_name=wrong&csrf_token={csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_name.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(wrong_name.into_body(), 128 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&body).contains("does not match"));

    let delete = app
        .clone()
        .oneshot(
            Request::post(format!("/stacks/definitions/{}/delete", stack.id))
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, "null")
                .header(COOKIE, authenticated)
                .body(Body::from(format!("confirm_name=api&csrf_token={csrf}")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(&path).unwrap();
    assert!(catalog.list_stacks().unwrap().is_empty());
}

#[tokio::test]
async fn overview_renders_readiness_and_dialog_and_serves_the_script() {
    let (_data, app) = app();
    let (authenticated, _csrf) = sign_in(&app).await;

    let dashboard = app
        .clone()
        .oneshot(
            Request::get("/")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dashboard.status(), StatusCode::OK);
    let body = to_bytes(dashboard.into_body(), 256 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("Host readiness"));
    assert!(body.contains("Docker"));
    assert!(body.contains("Compose"));
    assert!(body.contains("kubectl"));
    assert!(body.contains("Nixpacks"));
    assert!(body.contains("id=\"install-tool\""));
    assert!(body.contains("data-copy-command"));
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
    let body = to_bytes(script.into_body(), 64 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("delete-container"));
    assert!(body.contains("data-confirm-input"));
}

async fn post_form(
    app: &Router,
    path: &str,
    cookies: &str,
    origin: &str,
    form: String,
) -> Response {
    app.clone()
        .oneshot(
            Request::post(path)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(ORIGIN, origin)
                .header(COOKIE, cookies)
                .body(Body::from(form))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn body_text(response: Response, limit: usize) -> String {
    let body = to_bytes(response.into_body(), limit).await.unwrap();
    String::from_utf8_lossy(&body).into_owned()
}

#[tokio::test]
async fn container_creation_and_lifecycle_actions_are_guarded() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let provider_id = {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog
            .create_provider_connection("local", "docker", "unix:///var/run/docker.sock", "")
            .unwrap()
    };

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
                .header(COOKIE, authenticated.clone())
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
    assert!(body.contains(&provider_id));
    assert!(body.contains(&format!("value=\"{csrf}\"")));

    let create = format!(
        "provider_id={provider_id}&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token="
    );

    let no_csrf = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "null",
        format!("provider_id={provider_id}&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token=invalid"),
    )
    .await;
    assert_eq!(no_csrf.status(), StatusCode::FORBIDDEN);

    let bad_origin = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "http://attacker.example",
        format!("{create}{csrf}"),
    )
    .await;
    assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

    let missing_provider = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "null",
        format!("provider_id=missing&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(missing_provider.status(), StatusCode::NOT_FOUND);

    let bad_image = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "null",
        format!("provider_id={provider_id}&image=bad+image&name=demo-api&container_port=8080&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_image.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(bad_image, 128 * 1024)
        .await
        .contains("whitespace"));

    let bad_name = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "null",
        format!("provider_id={provider_id}&image=nginx%3Alatest&name=bad%2Fname&container_port=8080&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_name.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(bad_name, 128 * 1024).await.contains("letters"));

    let bad_port = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "null",
        format!("provider_id={provider_id}&image=nginx%3Alatest&name=demo-api&container_port=0&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_port.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(bad_port, 128 * 1024)
        .await
        .contains("Container port must be"));

    let created = post_form(
        &app,
        "/containers/new",
        &authenticated,
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

    let listed = app
        .clone()
        .oneshot(
            Request::get(&location)
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let body = body_text(listed, 256 * 1024).await;
    assert!(body.contains("is running on host port 18050."));
    assert!(body.contains(&format!("/containers/{provider_id}/demo-api/start")));
    assert!(body.contains(&format!("/containers/{provider_id}/demo-api/stop")));
    assert!(body.contains(&format!("/containers/{provider_id}/demo-api/restart")));
    assert!(body.contains(&format!(
        "data-delete-action=\"/containers/{provider_id}/demo-api/delete\""
    )));
    assert!(body.contains("data-delete-name=\"demo-api\""));
    assert!(body.contains("data-dialog-open=\"delete-container\""));
    assert!(body.contains("id=\"delete-container\""));
    assert!(body.contains("data-confirm-input"));
    assert!(body.contains("href=\"/containers/new\""));

    let stack = app
        .clone()
        .oneshot(
            Request::get(format!("/stacks/{provider_id}/hakobi-stack"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stack.status(), StatusCode::OK);
    let body = body_text(stack, 256 * 1024).await;
    assert!(body.contains(&format!("/containers/{provider_id}/demo-api/start")));
    assert!(body.contains("id=\"delete-container\""));

    for (action, word) in [
        ("start", "started"),
        ("stop", "stopped"),
        ("restart", "restarted"),
    ] {
        let forbidden = post_form(
            &app,
            &format!("/containers/{provider_id}/demo-api/{action}"),
            &authenticated,
            "null",
            "csrf_token=invalid".to_owned(),
        )
        .await;
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let bad_origin = post_form(
            &app,
            &format!("/containers/{provider_id}/demo-api/{action}"),
            &authenticated,
            "http://attacker.example",
            format!("csrf_token={csrf}"),
        )
        .await;
        assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

        let unknown = post_form(
            &app,
            &format!("/containers/missing/demo-api/{action}"),
            &authenticated,
            "null",
            format!("csrf_token={csrf}"),
        )
        .await;
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

        let invalid_name = post_form(
            &app,
            &format!("/containers/{provider_id}/bad%2Fname/{action}"),
            &authenticated,
            "null",
            format!("csrf_token={csrf}"),
        )
        .await;
        assert_eq!(invalid_name.status(), StatusCode::BAD_REQUEST);

        let ok = post_form(
            &app,
            &format!("/containers/{provider_id}/demo-api/{action}"),
            &authenticated,
            "null",
            format!("csrf_token={csrf}"),
        )
        .await;
        assert_eq!(ok.status(), StatusCode::SEE_OTHER);
        let location = ok.headers().get("location").unwrap().to_str().unwrap();
        assert!(location.contains(&format!("demo-api+{word}")));
    }

    let delete_without_csrf = post_form(
        &app,
        &format!("/containers/{provider_id}/demo-api/delete"),
        &authenticated,
        "null",
        "confirm_name=demo-api&csrf_token=invalid".to_owned(),
    )
    .await;
    assert_eq!(delete_without_csrf.status(), StatusCode::FORBIDDEN);

    let delete_wrong_name = post_form(
        &app,
        &format!("/containers/{provider_id}/demo-api/delete"),
        &authenticated,
        "null",
        format!("confirm_name=other&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(delete_wrong_name.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(delete_wrong_name, 64 * 1024)
        .await
        .contains("does not match"));

    let delete_unknown_provider = post_form(
        &app,
        "/containers/missing/demo-api/delete",
        &authenticated,
        "null",
        format!("confirm_name=demo-api&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(delete_unknown_provider.status(), StatusCode::NOT_FOUND);

    let deleted = post_form(
        &app,
        &format!("/containers/{provider_id}/demo-api/delete"),
        &authenticated,
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

    {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog.set_provider_enabled(&provider_id, false).unwrap();
    }
    let disabled = post_form(
        &app,
        "/containers/new",
        &authenticated,
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
