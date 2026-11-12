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

pub(crate) struct FakeBackend;

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


/// Backend that reports the container last started via `run_container` as
/// running with the deployed image, so the executor's verification passes.
pub(crate) struct PipelineBackend {
    deployed: tokio::sync::Mutex<Option<(String, String)>>,
}

impl PipelineBackend {
    pub(crate) fn new() -> Self {
        Self {
            deployed: tokio::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl ContainerBackend for PipelineBackend {
    async fn test(&self, _endpoint: &str) -> anyhow::Result<String> {
        Ok("Pipeline backend".into())
    }

    async fn containers(&self, _endpoint: &str) -> anyhow::Result<Vec<ContainerRow>> {
        let deployed = self.deployed.lock().await;
        Ok(match deployed.as_ref() {
            Some((name, image)) => vec![ContainerRow {
                id: "0123456789abcdef".into(),
                name: name.clone(),
                image: image.clone(),
                state: "running".into(),
                status: "Up 1 second".into(),
                health: None,
                compose_project: None,
                compose_service: None,
                ports: vec![],
            }],
            None => vec![],
        })
    }

    async fn snapshot(&self, _endpoint: &str) -> anyhow::Result<DockerSnapshot> {
        Ok(DockerSnapshot {
            host: HostInfo {
                name: "pipeline-test".into(),
                server_version: "28.0.1".into(),
                containers: 1,
                running: 1,
                images: 1,
                operating_system: "Test".into(),
                architecture: "x86_64".into(),
                cpus: 4,
            },
            images: vec![],
            volumes: vec![],
            networks: vec![],
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
        image: &str,
        name: &str,
        _container_port: u16,
    ) -> anyhow::Result<RunResult> {
        *self.deployed.lock().await = Some((name.to_owned(), image.to_owned()));
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

pub(crate) fn config() -> PublicConfig {
    PublicConfig {
        username: "admin".into(),
        password_hash: auth::hash_password("correct horse battery staple").unwrap(),
        base_url: "http://127.0.0.1:3000".into(),
        secure_cookies: false,
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
async fn root_redirects_to_dashboard() {
    let (_data, app) = app();
    let response = app
        .oneshot(Request::get("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/dashboard"
    );
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
            Request::get("/dashboard")
                .header(COOKIE, session.clone())
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
            Request::get("/dashboard")
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

pub(crate) async fn sign_in(app: &Router) -> (String, String) {
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
async fn overview_renders_readiness_and_dialog_and_serves_the_script() {
    let (_data, app) = app();
    let (authenticated, _csrf) = sign_in(&app).await;

    let dashboard = app
        .clone()
        .oneshot(
            Request::get("/dashboard")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dashboard.status(), StatusCode::OK);
    let body = to_bytes(dashboard.into_body(), 256 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
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
    let body = to_bytes(script.into_body(), 64 * 1024).await.unwrap();
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("delete-container"));
    assert!(body.contains("data-confirm-input"));
}

pub(crate) async fn post_form(
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

pub(crate) async fn body_text(response: Response, limit: usize) -> String {
    let body = to_bytes(response.into_body(), limit).await.unwrap();
    String::from_utf8_lossy(&body).into_owned()
}

#[tokio::test]
async fn container_creation_and_lifecycle_actions_are_guarded() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let provider = {
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
    assert!(body.contains(&provider));
    assert!(body.contains(&format!("value=\"{csrf}\"")));

    let create = format!(
        "provider_id={provider}&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token="
    );

    let no_csrf = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "null",
        format!("provider_id={provider}&image=nginx%3Alatest&name=demo-api&container_port=8080&csrf_token=invalid"),
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
        format!("provider_id={provider}&image=bad+image&name=demo-api&container_port=8080&csrf_token={csrf}"),
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
        format!("provider_id={provider}&image=nginx%3Alatest&name=bad%2Fname&container_port=8080&csrf_token={csrf}"),
    )
    .await;
    assert_eq!(bad_name.status(), StatusCode::BAD_REQUEST);
    assert!(body_text(bad_name, 128 * 1024).await.contains("letters"));

    let bad_port = post_form(
        &app,
        "/containers/new",
        &authenticated,
        "null",
        format!("provider_id={provider}&image=nginx%3Alatest&name=demo-api&container_port=0&csrf_token={csrf}"),
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

for (action, word) in [
        ("start", "started"),
        ("stop", "stopped"),
        ("restart", "restarted"),
    ] {
        let forbidden = post_form(
            &app,
            &format!("/containers/{provider}/demo-api/{action}"),
            &authenticated,
            "null",
            "csrf_token=invalid".to_owned(),
        )
        .await;
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let bad_origin = post_form(
            &app,
            &format!("/containers/{provider}/demo-api/{action}"),
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
            &format!("/containers/{provider}/bad%2Fname/{action}"),
            &authenticated,
            "null",
            format!("csrf_token={csrf}"),
        )
        .await;
        assert_eq!(invalid_name.status(), StatusCode::BAD_REQUEST);

        let ok = post_form(
            &app,
            &format!("/containers/{provider}/demo-api/{action}"),
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
        &format!("/containers/{provider}/demo-api/delete"),
        &authenticated,
        "null",
        "confirm_name=demo-api&csrf_token=invalid".to_owned(),
    )
    .await;
    assert_eq!(delete_without_csrf.status(), StatusCode::FORBIDDEN);

    let delete_wrong_name = post_form(
        &app,
        &format!("/containers/{provider}/demo-api/delete"),
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
        &format!("/containers/{provider}/demo-api/delete"),
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
        catalog.set_provider_enabled(&provider, false).unwrap();
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

/// Fails every call so a test can prove which backend served a request.
// ─── Host enrollment tests ────────────────────────────────────────────────

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
#[tokio::test]
async fn apps_can_be_created_edited_and_deleted_over_http() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let host_id = {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog
            .create_host_agent(
                "env-http-host",
                "https://env-http.example.com",
                "tok_env_http_01",
            )
            .unwrap()
    };

    // Empty state renders with CTA.
    let list = app
        .clone()
        .oneshot(
            Request::get("/apps")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let body = body_text(list, 128 * 1024).await;
    assert!(body.contains("Create your first app"));

    // New form renders host select and CSRF field.
    let new_page = app
        .clone()
        .oneshot(
            Request::get("/apps/new")
                .header(COOKIE, authenticated.clone())
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

    // Server-side validation: bad build type.
    let invalid = post_form(
        &app,
        "/apps",
        &authenticated,
        "null",
        format!(
            "name=web&host_id={host_id}&build_type=helm&github_repo=example%2Frepo&github_branch=main&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    // Server-side validation: container port out of range.
    let invalid = post_form(
        &app,
        "/apps",
        &authenticated,
        "null",
        format!(
            "name=web&host_id={host_id}&build_type=compose&github_repo=&github_branch=main&container_port=0&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    // Invalid host id returns 400 (rendered as form error).
    let invalid_host = post_form(
        &app,
        "/apps",
        &authenticated,
        "null",
        format!(
            "name=web&host_id=no-such-host&build_type=compose&github_repo=&github_branch=main&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(invalid_host.status(), StatusCode::BAD_REQUEST);

    // Create succeeds.
    let create = post_form(
        &app,
        "/apps",
        &authenticated,
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

    let app_id = {
        let catalog = Catalog::open(&path).unwrap();
        let apps = catalog.list_apps().unwrap();
        assert_eq!(apps.len(), 1);
        apps[0].id.clone()
    };

    // List shows the app.
    let list = app
        .clone()
        .oneshot(
            Request::get("/apps")
                .header(COOKIE, authenticated.clone())
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

    // Detail page shows metadata and actions.
    let detail = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}"))
                .header(COOKIE, authenticated.clone())
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

    // Edit form is pre-filled.
    let edit_page = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/edit"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(edit_page.status(), StatusCode::OK);
    let body = body_text(edit_page, 128 * 1024).await;
    assert!(body.contains("value=\"web\""));
    assert!(body.contains("Edit app"));

    // Edit submits.
    let update = post_form(
        &app,
        &format!("/apps/{app_id}/edit"),
        &authenticated,
        "null",
        format!(
            "name=web-2&host_id={host_id}&build_type=dockerfile&github_repo=&github_branch=&github_dockerfile=Dockerfile&csrf_token={csrf}"
        ),
    )
    .await;
    assert_eq!(update.status(), StatusCode::SEE_OTHER);
    {
        let catalog = Catalog::open(&path).unwrap();
        let updated = catalog.get_app(&app_id).unwrap().unwrap();
        assert_eq!(updated.name, "web-2");
        assert_eq!(updated.build_type, "dockerfile");
        assert!(updated.github_repo.is_none());
        assert!(updated.github_branch.is_none());
        assert_eq!(updated.github_dockerfile.as_deref(), Some("Dockerfile"));
        assert!(!updated.github_auto_deploy);
        assert!(updated.container_port.is_none());
    }

    // CP6: deploy queues a pipeline run (no build runner configured, so the
    // executor marks it failed asynchronously) and redirects with a notice.
    let deploy = post_form(
        &app,
        &format!("/apps/{app_id}/deploy"),
        &authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(deploy.status(), StatusCode::SEE_OTHER);

    // CP6: rollback with no running deployment redirects with a no-op notice.
    let rollback = post_form(
        &app,
        &format!("/apps/{app_id}/rollback"),
        &authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(rollback.status(), StatusCode::SEE_OTHER);

    // Deployments and logs pages render empty states.
    let deployments = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/deployments"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deployments.status(), StatusCode::OK);
    // CP6: the queued deploy created a real deployment record, so the page
    // shows it rather than the empty state.
    assert!(!body_text(deployments, 128 * 1024).await.contains("No deployments"));

    let logs = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/logs"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs.status(), StatusCode::OK);
    // CP6: the queued deploy writes stage logs, so the page is not empty.
    assert!(!body_text(logs, 128 * 1024).await.contains("No log lines"));

    // Unknown app 404s.
    let missing = app
        .clone()
        .oneshot(
            Request::get("/apps/nope")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // Unauthorized POST is rejected.
    let forbidden = post_form(
        &app,
        "/apps",
        &authenticated,
        "https://evil.example.com",
        format!(
            "name=evil&host_id={host_id}&build_type=compose&github_repo=&github_branch=&csrf_token=wrong"
        ),
    )
    .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    // Delete removes the app.
    let delete = post_form(
        &app,
        &format!("/apps/{app_id}/delete"),
        &authenticated,
        "null",
        format!("csrf_token={csrf}"),
    )
    .await;
    assert_eq!(delete.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(&path).unwrap();
    assert!(catalog.list_apps().unwrap().is_empty());
}

#[tokio::test]
async fn app_status_partial_refreshes_with_htmx() {
    let (data, app) = app();
    let (authenticated, csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let (host_id, app_id) = {
        let mut catalog = Catalog::open(&path).unwrap();
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
    };

    let partial = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/status"))
                .header(COOKIE, authenticated.clone())
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

    // Deployment records appear in the partial once created.
    {
        let mut catalog = Catalog::open(&path).unwrap();
        let deployment_id = catalog
            .create_deployment(&app_id, "manual", None, None)
            .unwrap();
        catalog
            .update_deployment_status(&deployment_id, "running", None)
            .unwrap();
        catalog
            .add_deployment_log(&deployment_id, "info", "deployed v1")
            .unwrap();
    }
    let partial = app
        .clone()
        .oneshot(
            Request::get(format!("/apps/{app_id}/status"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_text(partial, 64 * 1024).await;
    assert!(body.contains("running"));
    assert!(body.contains("v1"));

    let _ = host_id;
    let _ = csrf;
}

/// Backend that answers the container detail reads with fixed data, so the
/// detail page, stats partial, and logs partial can be rendered without a
/// Docker daemon.
struct DetailBackend;

fn detail_inspect() -> hakobi::providers::DockerInspect {
    hakobi::providers::DockerInspect {
        id: "0123456789abcdef".into(),
        name: "demo-api".into(),
        image: "hakobi/demo:latest".into(),
        image_id: "sha256:abcdef".into(),
        command: "/app/server --port 8080".into(),
        state: "running".into(),
        status: String::new(),
        health: Some("healthy".into()),
        created_at: "2026-01-01T00:00:00Z".into(),
        started_at: "2026-01-02T00:00:00Z".into(),
        restart_count: 1,
        platform: "linux".into(),
        ports: vec![ContainerPort {
            host_ip: Some("0.0.0.0".into()),
            host_port: Some(18080),
            container_port: 8080,
            protocol: "tcp".into(),
        }],
        mounts: vec![hakobi::providers::ContainerMount {
            source: "/var/lib/docker/volumes/data/_data".into(),
            destination: "/data".into(),
            mode: String::new(),
            read_only: false,
        }],
        env: vec!["RUST_LOG=info".into()],
        labels: vec![(
            "com.docker.compose.project".to_owned(),
            "hakobi-stack".to_owned(),
        )],
    }
}

#[async_trait]
impl ContainerBackend for DetailBackend {
    async fn test(&self, _endpoint: &str) -> anyhow::Result<String> {
        Ok("Detail backend".into())
    }

    async fn containers(&self, _endpoint: &str) -> anyhow::Result<Vec<ContainerRow>> {
        Ok(vec![ContainerRow {
            id: "0123456789abcdef".into(),
            name: "demo-api".into(),
            image: "hakobi/demo:latest".into(),
            state: "running".into(),
            status: "Up 2 hours".into(),
            health: Some("healthy".into()),
            compose_project: None,
            compose_service: None,
            ports: vec![],
        }])
    }

    async fn snapshot(&self, _endpoint: &str) -> anyhow::Result<DockerSnapshot> {
        Ok(DockerSnapshot {
            host: HostInfo {
                name: "detail-test".into(),
                server_version: "28.0.1".into(),
                containers: 1,
                running: 1,
                images: 1,
                operating_system: "Test".into(),
                architecture: "x86_64".into(),
                cpus: 4,
            },
            images: vec![],
            volumes: vec![],
            networks: vec![],
        })
    }

    async fn inspect_container(
        &self,
        _endpoint: &str,
        name: &str,
    ) -> anyhow::Result<hakobi::providers::DockerInspect> {
        if name == "demo-api" {
            Ok(detail_inspect())
        } else {
            Err(anyhow::anyhow!(
                "Docker responded with status code 404: No such container: {name}"
            ))
        }
    }

    async fn container_logs(
        &self,
        _endpoint: &str,
        name: &str,
        tail: usize,
    ) -> anyhow::Result<hakobi::providers::DockerLogs> {
        if name != "demo-api" {
            return Err(anyhow::anyhow!("No such container: {name}"));
        }
        let count = tail.min(3);
        Ok(hakobi::providers::DockerLogs {
            lines: (0..count).map(|index| format!("line-{index}")).collect(),
            truncated: false,
        })
    }

    async fn container_stats(
        &self,
        _endpoint: &str,
        name: &str,
    ) -> anyhow::Result<hakobi::providers::DockerContainerUsage> {
        if name != "demo-api" {
            return Err(anyhow::anyhow!("No such container: {name}"));
        }
        Ok(hakobi::providers::DockerContainerUsage {
            cpu_percent: 12.5,
            memory_used_bytes: 1_048_576,
            memory_limit_bytes: 8_388_608,
            network_rx_bytes: 4096,
            network_tx_bytes: 8192,
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

fn detail_app() -> (TempDir, Router) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.sqlite3");
    (
        directory,
        router_with_backend(config(), path, Arc::new(DetailBackend)),
    )
}

#[tokio::test]
async fn container_detail_page_renders_metadata_stats_and_logs() {
    let (data, app) = detail_app();
    let (authenticated, _csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let provider = {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog
            .create_provider_connection("local", "docker", "unix:///var/run/docker.sock", "")
            .unwrap()
    };

    let anonymous = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);

    let page = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = body_text(page, 256 * 1024).await;
    assert!(body.contains("demo-api"));
    assert!(body.contains("hakobi/demo:latest"));
    assert!(body.contains("healthy"));
    assert!(body.contains("/app/server --port 8080"));
    assert!(body.contains("RUST_LOG=info"));
    assert!(body.contains("com.docker.compose.project=hakobi-stack"));
    assert!(body.contains("0.0.0.0:18080"));
    assert!(body.contains("12.5 %"));
    assert!(body.contains("line-0"));
    // HTMX poll wiring for the two live sections.
    assert!(body.contains(
        format!("hx-get=\"/containers/{provider}/demo-api/stats\" hx-trigger=\"every 2s\"").as_str()
    ));
    assert!(body.contains(
        format!("hx-get=\"/containers/{provider}/demo-api/logs\" hx-trigger=\"every 2s\"").as_str()
    ));
    // Lifecycle actions plus typed-name delete confirmation.
    assert!(body.contains(&format!("/containers/{provider}/demo-api/start")));
    assert!(body.contains(&format!("/containers/{provider}/demo-api/stop")));
    assert!(body.contains(&format!("/containers/{provider}/demo-api/restart")));
    assert!(body.contains(&format!(
        "data-delete-action=\"/containers/{provider}/demo-api/delete\""
    )));
    assert!(body.contains("data-delete-name=\"demo-api\""));
    assert!(body.contains("data-confirm-input"));
    // Link from the containers list to the detail page.
    let list = app
        .clone()
        .oneshot(
            Request::get("/containers")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = body_text(list, 256 * 1024).await;
    assert!(list_body.contains(&format!("href=\"/containers/{provider}/demo-api\"")));
}

#[tokio::test]
async fn container_detail_partials_return_stats_and_logs() {
    let (data, app) = detail_app();
    let (authenticated, _csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let provider = {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog
            .create_provider_connection("local", "docker", "unix:///var/run/docker.sock", "")
            .unwrap()
    };

    let stats = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api/stats"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stats.status(), StatusCode::OK);
    let stats_body = body_text(stats, 64 * 1024).await;
    assert!(stats_body.contains("Resource usage"));
    assert!(stats_body.contains("12.5 %"));
    assert!(stats_body.contains("1.0 MiB"));

    let logs = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api/logs"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs.status(), StatusCode::OK);
    let logs_body = body_text(logs, 64 * 1024).await;
    assert!(logs_body.contains("line-0"));
    assert!(logs_body.contains("line-2"));

    let anonymous = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api/logs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        anonymous.headers().get("hx-redirect").unwrap(),
        "/login"
    );
}

#[tokio::test]
async fn container_detail_returns_404_for_missing_containers_and_providers() {
    let (data, app) = detail_app();
    let (authenticated, _csrf) = sign_in(&app).await;
    let path = data.path().join("catalog.sqlite3");
    let provider = {
        let mut catalog = Catalog::open(&path).unwrap();
        catalog
            .create_provider_connection("local", "docker", "unix:///var/run/docker.sock", "")
            .unwrap()
    };

    let missing_container = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/ghost-api"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_container.status(), StatusCode::NOT_FOUND);

    let missing_provider = app
        .clone()
        .oneshot(
            Request::get("/containers/missing/demo-api")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_provider.status(), StatusCode::NOT_FOUND);

    let bad_name = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/bad%2Fname"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_name.status(), StatusCode::BAD_REQUEST);
}
