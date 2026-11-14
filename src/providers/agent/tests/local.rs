use tempfile::TempDir;

use crate::catalog::Catalog;
use crate::host_agent::{self, LOCAL_PROVIDER_KIND, LOCAL_PROVIDER_NAME};
use crate::providers::agent::{ensure_local_provider, LocalDockerAccess};

fn catalog() -> (TempDir, Catalog) {
    let directory = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(directory.path().join("catalog.sqlite3")).unwrap();
    (directory, catalog)
}

#[test]
fn local_provider_registration_is_idempotent() {
    let (_directory, mut catalog) = catalog();
    let first = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
        .unwrap()
        .expect("provider registered");
    let second = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
        .unwrap()
        .expect("provider reused");
    assert_eq!(first, second);

    let providers = catalog.list_provider_connections().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].name, LOCAL_PROVIDER_NAME);
    assert_eq!(providers[0].kind, LOCAL_PROVIDER_KIND);
    assert_eq!(providers[0].endpoint, host_agent::LOCAL_ENDPOINT);
    assert!(providers[0].enabled);
}

#[test]
fn local_provider_registration_uses_the_socket_without_an_agent() {
    let (_directory, mut catalog) = catalog();
    ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket)).unwrap();
    let providers = catalog.list_provider_connections().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].endpoint, host_agent::DOCKER_SOCKET_ENDPOINT);
    assert_eq!(providers[0].name, LOCAL_PROVIDER_NAME);
}

#[test]
fn existing_provider_rows_are_preserved() {
    let (_directory, mut catalog) = catalog();
    let existing = catalog
        .create_provider_connection(
            "Primary docker",
            "docker",
            host_agent::DOCKER_SOCKET_ENDPOINT,
            "unused-secret",
        )
        .unwrap();

    // The socket path is already covered, so no second row appears and the
    // operator's row keeps its name, endpoint, and credential.
    let registered =
        ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket)).unwrap();
    assert_eq!(registered.as_deref(), Some(existing.as_str()));

    let providers = catalog.list_provider_connections().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].name, "Primary docker");
    assert_eq!(providers[0].endpoint, host_agent::DOCKER_SOCKET_ENDPOINT);
    assert_eq!(catalog.provider_secret(&existing).unwrap(), "unused-secret");
}

#[test]
fn switching_to_the_host_agent_adds_one_provider_without_touching_others() {
    let (_directory, mut catalog) = catalog();
    catalog
        .create_provider_connection(
            "Primary docker",
            "docker",
            host_agent::DOCKER_SOCKET_ENDPOINT,
            "unused-secret",
        )
        .unwrap();

    let id = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
        .unwrap()
        .expect("agent provider registered");
    let providers = catalog.list_provider_connections().unwrap();
    assert_eq!(providers.len(), 2);
    assert!(providers
        .iter()
        .any(|provider| provider.name == "Primary docker"
            && provider.endpoint == host_agent::DOCKER_SOCKET_ENDPOINT));
    let agent = providers
        .iter()
        .find(|provider| provider.id == id)
        .expect("agent provider present");
    assert_eq!(agent.name, LOCAL_PROVIDER_NAME);
    assert_eq!(agent.endpoint, host_agent::LOCAL_ENDPOINT);

    // The next start reuses the row instead of adding another one.
    let again = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent)).unwrap();
    assert_eq!(again.as_deref(), Some(id.as_str()));
    assert_eq!(catalog.list_provider_connections().unwrap().len(), 2);
}

#[test]
fn an_existing_managed_name_is_never_duplicated() {
    let (_directory, mut catalog) = catalog();
    catalog
        .create_provider_connection(
            LOCAL_PROVIDER_NAME,
            "docker",
            "tcp://127.0.0.1:2375",
            "unused-secret",
        )
        .unwrap();

    assert!(
        ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
            .unwrap()
            .is_none()
    );
    let providers = catalog.list_provider_connections().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].endpoint, "tcp://127.0.0.1:2375");
}

#[test]
fn a_disabled_local_provider_is_not_reenabled() {
    let (_directory, mut catalog) = catalog();
    let id = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket))
        .unwrap()
        .expect("provider registered");
    catalog.set_provider_enabled(&id, false).unwrap();

    ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket)).unwrap();
    let providers = catalog.list_provider_connections().unwrap();
    assert_eq!(providers.len(), 1);
    assert!(!providers[0].enabled);
}

#[test]
fn no_provider_is_registered_without_an_agent_or_socket() {
    let (_directory, mut catalog) = catalog();
    assert!(ensure_local_provider(&mut catalog, None).unwrap().is_none());
    assert!(catalog.list_provider_connections().unwrap().is_empty());
}

#[test]
fn local_access_endpoints_are_distinct() {
    assert_eq!(
        LocalDockerAccess::HostAgent.endpoint(),
        host_agent::LOCAL_ENDPOINT
    );
    assert_eq!(
        LocalDockerAccess::DockerSocket.endpoint(),
        host_agent::DOCKER_SOCKET_ENDPOINT
    );
}
