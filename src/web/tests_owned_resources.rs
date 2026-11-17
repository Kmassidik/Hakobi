use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header::COOKIE, Request, StatusCode},
    Router,
};
use hakobi::{
    catalog::Catalog,
    providers::{
        ContainerBackend, ContainerRow, DockerSnapshot, HostInfo, NetworkRow, ResourceLimits,
        RunResult, VolumeRow,
    },
};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

use super::{
    router_with_backend,
    tests_support::{assert_error, body_text, config, post_form, sign_in, FakeBackend},
};

#[derive(Default)]
struct Recorded {
    events: Vec<String>,
    creates: Vec<(String, String, Option<String>)>,
    networks: Vec<String>,
    volumes: Vec<String>,
}

struct RecordingBackend {
    fake: FakeBackend,
    snapshot: DockerSnapshot,
    recorded: Mutex<Recorded>,
}

impl RecordingBackend {
    fn new(app_id: &str, other_app_id: &str) -> Self {
        Self {
            fake: FakeBackend,
            snapshot: DockerSnapshot {
                host: host_info(),
                images: vec![],
                networks: vec![
                    network("owned-net", Some(app_id)),
                    network("other-net", Some(other_app_id)),
                    network("shared-net", None),
                ],
                volumes: vec![
                    volume("owned-vol", Some(app_id)),
                    volume("other-vol", Some(other_app_id)),
                    volume("shared-vol", None),
                ],
            },
            recorded: Mutex::new(Recorded::default()),
        }
    }

    async fn event(&self, event: String) {
        self.recorded.lock().await.events.push(event);
    }
}

#[async_trait]
impl ContainerBackend for RecordingBackend {
    async fn test(&self, endpoint: &str) -> anyhow::Result<String> {
        self.fake.test(endpoint).await
    }

    async fn containers(&self, endpoint: &str) -> anyhow::Result<Vec<ContainerRow>> {
        self.fake.containers(endpoint).await
    }

    async fn snapshot(&self, endpoint: &str) -> anyhow::Result<DockerSnapshot> {
        self.event(format!("snapshot:{endpoint}")).await;
        Ok(self.snapshot.clone())
    }

    async fn start_container(&self, endpoint: &str, name: &str) -> anyhow::Result<()> {
        self.fake.start_container(endpoint, name).await
    }

    async fn stop_container(&self, endpoint: &str, name: &str) -> anyhow::Result<()> {
        self.fake.stop_container(endpoint, name).await
    }

    async fn restart_container(&self, endpoint: &str, name: &str) -> anyhow::Result<()> {
        self.fake.restart_container(endpoint, name).await
    }

    async fn pause_container(&self, endpoint: &str, name: &str) -> anyhow::Result<()> {
        self.fake.pause_container(endpoint, name).await
    }

    async fn unpause_container(&self, endpoint: &str, name: &str) -> anyhow::Result<()> {
        self.fake.unpause_container(endpoint, name).await
    }

    async fn remove_container(&self, endpoint: &str, name: &str) -> anyhow::Result<()> {
        self.event(format!("container:{name}")).await;
        self.fake.remove_container(endpoint, name).await
    }

    async fn remove_image(&self, endpoint: &str, image: &str) -> anyhow::Result<()> {
        self.fake.remove_image(endpoint, image).await
    }

    async fn create_network(
        &self,
        _endpoint: &str,
        name: &str,
        app_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.recorded.lock().await.creates.push((
            "network".into(),
            name.into(),
            app_id.map(str::to_owned),
        ));
        Ok(())
    }

    async fn remove_network(&self, _endpoint: &str, name: &str) -> anyhow::Result<()> {
        let mut recorded = self.recorded.lock().await;
        recorded.events.push(format!("network:{name}"));
        recorded.networks.push(name.into());
        Ok(())
    }

    async fn create_volume(
        &self,
        _endpoint: &str,
        name: &str,
        app_id: Option<&str>,
    ) -> anyhow::Result<()> {
        self.recorded.lock().await.creates.push((
            "volume".into(),
            name.into(),
            app_id.map(str::to_owned),
        ));
        Ok(())
    }

    async fn remove_volume(&self, _endpoint: &str, name: &str) -> anyhow::Result<()> {
        let mut recorded = self.recorded.lock().await;
        recorded.events.push(format!("volume:{name}"));
        recorded.volumes.push(name.into());
        Ok(())
    }

    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        port: u16,
        requested_host_port: Option<u16>,
        env: &std::collections::HashMap<String, String>,
        resources: ResourceLimits,
    ) -> anyhow::Result<RunResult> {
        self.fake
            .run_container(
                endpoint,
                image,
                name,
                port,
                requested_host_port,
                env,
                resources,
            )
            .await
    }
}

struct Fixture {
    _data: TempDir,
    path: PathBuf,
    router: Router,
    backend: Arc<RecordingBackend>,
    provider: String,
    host: String,
    app_id: String,
    other_app_id: String,
    remote_app_id: String,
}

fn fixture() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).unwrap();
    let host = catalog
        .create_host_agent("owned-host", "https://owned.example.com", "tok_owned_01")
        .unwrap();
    let remote_host = catalog
        .create_host_agent("remote-host", "https://remote.example.com", "tok_remote_01")
        .unwrap();
    let provider = catalog.create_host_agent_provider(&host).unwrap();
    let app_id = create_app(&mut catalog, &host, "owned-app");
    let other_app_id = create_app(&mut catalog, &host, "other-app");
    let remote_app_id = create_app(&mut catalog, &remote_host, "remote-app");
    drop(catalog);
    let backend = Arc::new(RecordingBackend::new(&app_id, &other_app_id));
    let router = router_with_backend(config(), path.clone(), backend.clone());
    Fixture {
        _data: data,
        path,
        router,
        backend,
        provider,
        host,
        app_id,
        other_app_id,
        remote_app_id,
    }
}

fn create_app(catalog: &mut Catalog, host_id: &str, name: &str) -> String {
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

fn host_info() -> HostInfo {
    HostInfo {
        name: "recording".into(),
        server_version: "1".into(),
        containers: 0,
        running: 0,
        images: 0,
        operating_system: "test".into(),
        architecture: "test".into(),
        cpus: 1,
    }
}

fn network(name: &str, app_id: Option<&str>) -> NetworkRow {
    NetworkRow {
        name: name.into(),
        driver: "bridge".into(),
        scope: "local".into(),
        app_id: app_id.map(str::to_owned),
    }
}

fn volume(name: &str, app_id: Option<&str>) -> VolumeRow {
    VolumeRow {
        name: name.into(),
        driver: "local".into(),
        mountpoint: format!("/data/{name}"),
        app_id: app_id.map(str::to_owned),
    }
}

async fn page(router: &Router, authenticated: &str, uri: &str) -> String {
    let response = router
        .clone()
        .oneshot(
            Request::get(uri)
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_text(response, 256 * 1024).await
}

async fn submit_create(
    fixture: &Fixture,
    authenticated: &str,
    csrf: &str,
    resource: &str,
    name: &str,
    app_id: &str,
) -> axum::response::Response {
    post_form(
        &fixture.router,
        &format!("/docker/{resource}/create"),
        authenticated,
        "null",
        format!(
            "provider_id={}&name={name}&app_id={app_id}&csrf_token={csrf}",
            fixture.provider
        ),
    )
    .await
}

#[tokio::test]
async fn owner_options_and_creates_are_restricted_to_the_provider_host() {
    let fixture = fixture();
    let (authenticated, csrf) = sign_in(&fixture.router).await;
    let body = page(&fixture.router, &authenticated, "/containers").await;
    assert!(body.contains("Unowned"));
    assert!(body.contains(&format!("value=\"{}\"", fixture.app_id)));
    assert!(body.contains("data-resource-owner=\"owned-app\""));
    assert!(!body.contains("remote-app"));
    for (resource, name, app_id) in [
        ("networks", "created-net", fixture.app_id.as_str()),
        ("networks", "unowned-net", ""),
        ("volumes", "created-vol", fixture.app_id.as_str()),
        ("volumes", "unowned-vol", ""),
    ] {
        let response = submit_create(&fixture, &authenticated, &csrf, resource, name, app_id).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
    }
    for app_id in [fixture.remote_app_id.as_str(), "missing"] {
        let response = submit_create(
            &fixture,
            &authenticated,
            &csrf,
            "networks",
            "rejected-net",
            app_id,
        )
        .await;
        assert_error(&response, "Owner+app+must+exist");
    }
    let recorded = fixture.backend.recorded.lock().await;
    assert_eq!(
        recorded.creates,
        vec![
            (
                "network".into(),
                "created-net".into(),
                Some(fixture.app_id.clone())
            ),
            ("network".into(), "unowned-net".into(), None),
            (
                "volume".into(),
                "created-vol".into(),
                Some(fixture.app_id.clone())
            ),
            ("volume".into(), "unowned-vol".into(), None),
        ]
    );
}

async fn delete_owned_app(fixture: &Fixture, delete_data: bool) {
    let (authenticated, csrf) = sign_in(&fixture.router).await;
    let consent = if delete_data { "delete_data=on&" } else { "" };
    let response = post_form(
        &fixture.router,
        &format!("/apps/{}/delete", fixture.app_id),
        &authenticated,
        "null",
        format!("confirm_name=owned-app&{consent}csrf_token={csrf}"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let catalog = Catalog::open(&fixture.path).unwrap();
    assert!(catalog.get_app(&fixture.app_id).unwrap().is_none());
    assert!(catalog.get_app(&fixture.other_app_id).unwrap().is_some());
}

fn assert_snapshot_precedes_cleanup(fixture: &Fixture, events: &[String]) {
    let snapshot = events
        .iter()
        .position(|event| event == &format!("snapshot:agent://{}", fixture.host))
        .unwrap();
    let container = events
        .iter()
        .position(|event| event == &format!("container:hakobi-{}", fixture.app_id))
        .unwrap();
    let network = events
        .iter()
        .position(|event| event == "network:owned-net")
        .unwrap();
    assert!(snapshot < container && container < network, "{events:?}");
}

#[tokio::test]
async fn app_delete_removes_only_owned_networks_and_preserves_volumes() {
    let fixture = fixture();
    delete_owned_app(&fixture, false).await;
    let recorded = fixture.backend.recorded.lock().await;
    assert_eq!(recorded.networks, vec!["owned-net"]);
    assert!(recorded.volumes.is_empty());
    assert_snapshot_precedes_cleanup(&fixture, &recorded.events);
}

#[tokio::test]
async fn app_delete_with_consent_removes_only_owned_networks_and_volumes() {
    let fixture = fixture();
    delete_owned_app(&fixture, true).await;
    let recorded = fixture.backend.recorded.lock().await;
    assert_eq!(recorded.networks, vec!["owned-net"]);
    assert_eq!(recorded.volumes, vec!["owned-vol"]);
    assert_snapshot_precedes_cleanup(&fixture, &recorded.events);
}
