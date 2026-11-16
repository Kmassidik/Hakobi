use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::providers::agent::RoutingBackend;
use crate::providers::docker::{
    ContainerBackend, ContainerRow, DockerSnapshot, ResourceLimits, RunResult,
};

/// Backend that records which endpoint it was asked to serve.
struct StubBackend(&'static str);

#[async_trait]
impl ContainerBackend for StubBackend {
    async fn test(&self, _endpoint: &str) -> Result<String> {
        Ok(format!("{} test", self.0))
    }

    async fn containers(&self, _endpoint: &str) -> Result<Vec<ContainerRow>> {
        Ok(Vec::new())
    }

    async fn snapshot(&self, _endpoint: &str) -> Result<DockerSnapshot> {
        bail!("{} snapshot", self.0)
    }

    async fn start_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
        Ok(())
    }

    async fn stop_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
        Ok(())
    }

    async fn restart_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
        Ok(())
    }

    async fn pause_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
        bail!("{} pause", self.0)
    }

    async fn unpause_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
        bail!("{} unpause", self.0)
    }

    async fn remove_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
        Ok(())
    }

    async fn remove_image(&self, _endpoint: &str, _image: &str) -> Result<()> {
        bail!("{} remove image", self.0)
    }

    async fn create_network(
        &self,
        _endpoint: &str,
        _name: &str,
        _app_id: Option<&str>,
    ) -> Result<()> {
        bail!("{} create network", self.0)
    }

    async fn remove_network(&self, _endpoint: &str, _name: &str) -> Result<()> {
        bail!("{} remove network", self.0)
    }

    async fn create_volume(
        &self,
        _endpoint: &str,
        _name: &str,
        _app_id: Option<&str>,
    ) -> Result<()> {
        bail!("{} create volume", self.0)
    }

    async fn remove_volume(&self, _endpoint: &str, _name: &str) -> Result<()> {
        bail!("{} remove volume", self.0)
    }

    async fn run_container(
        &self,
        _endpoint: &str,
        _image: &str,
        _name: &str,
        _container_port: u16,
        _env: &std::collections::HashMap<String, String>,
        _resources: ResourceLimits,
    ) -> Result<RunResult> {
        bail!("{} run", self.0)
    }
}

fn routing(agent: Option<&'static str>) -> RoutingBackend {
    RoutingBackend::new(
        Arc::new(StubBackend("docker")),
        agent.map(|label| Arc::new(StubBackend(label)) as Arc<dyn ContainerBackend>),
        None,
    )
}

#[tokio::test]
async fn routing_dispatches_by_endpoint_scheme() {
    let backend = routing(Some("agent"));
    assert_eq!(backend.test("agent://local").await.unwrap(), "agent test");
    assert_eq!(
        backend.test("unix:///var/run/docker.sock").await.unwrap(),
        "docker test"
    );
    assert_eq!(
        backend.test("tcp://127.0.0.1:2375").await.unwrap(),
        "docker test"
    );
    assert_eq!(
        backend
            .pause_container("agent://local", "demo")
            .await
            .unwrap_err()
            .to_string(),
        "agent pause"
    );
    assert_eq!(
        backend
            .unpause_container("unix:///var/run/docker.sock", "demo")
            .await
            .unwrap_err()
            .to_string(),
        "docker unpause"
    );
    assert_eq!(
        backend
            .remove_image("agent://local", "alpine:3.20")
            .await
            .unwrap_err()
            .to_string(),
        "agent remove image"
    );
    assert!(backend.snapshot("agent://local").await.is_err());
    assert!(backend
        .run_container(
            "agent://local",
            "alpine:3.20",
            "demo",
            80,
            &std::collections::HashMap::new(),
            ResourceLimits::default(),
        )
        .await
        .is_err());
}

#[tokio::test]
async fn routing_without_an_agent_reports_a_configuration_error() {
    let backend = routing(None);
    let error = backend.test("agent://local").await.unwrap_err().to_string();
    assert!(error.contains("HAKOBI_AGENT_URL"), "{error}");
    assert_eq!(
        backend.test("unix:///var/run/docker.sock").await.unwrap(),
        "docker test"
    );
}
