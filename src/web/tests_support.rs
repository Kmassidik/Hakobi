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
use hakobi::providers::{
    ContainerBackend, ContainerPort, ContainerRow, DockerSnapshot, HostInfo, ImageRow, NetworkRow,
    RunResult, VolumeRow,
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
        _env: &std::collections::HashMap<String, String>,
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
        _env: &std::collections::HashMap<String, String>,
    ) -> anyhow::Result<RunResult> {
        *self.deployed.lock().await = Some((name.to_owned(), image.to_owned()));
        Ok(RunResult {
            container_id: "fedcba9876543210".into(),
            host_port: 18050,
            name: name.to_owned(),
        })
    }
}

pub(crate) fn app() -> (TempDir, Router) {
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

pub(crate) fn cookie(response: &Response, name: &str) -> String {
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

pub(crate) fn form_encode(value: &str) -> String {
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
