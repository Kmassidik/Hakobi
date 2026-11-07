use hakobi::catalog::Catalog;

#[test]
fn catalog_persists_hierarchy_across_reopen() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");

    let service_id;
    {
        let mut catalog = Catalog::open(&path).expect("open catalog");
        assert_eq!(catalog.schema_version().expect("schema version"), 4);

        let organization_id = catalog
            .create_organization("default", "Default")
            .expect("create organization");
        let project_id = catalog
            .create_project(&organization_id, "hakobi", "Hakobi")
            .expect("create project");
        let environment_id = catalog
            .create_environment(&project_id, "production", "Production")
            .expect("create environment");
        service_id = catalog
            .create_service(&environment_id, "web", "Web")
            .expect("create service");

        let services = catalog.list_services().expect("list services");
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].id, service_id);
        assert_eq!(services[0].slug, "web");
        assert_eq!(services[0].name, "Web");
        assert_eq!(services[0].environment_id, environment_id);
        assert!(!services[0].created_at.is_empty());

        let audit = catalog.list_audit_events().expect("list audit events");
        assert!(audit.len() >= 4);
        assert!(audit.iter().any(|event| {
            event.action == "create"
                && event.entity_kind == "service"
                && event.entity_id == service_id
                && event.actor == "system"
        }));
    }

    let catalog = Catalog::open(&path).expect("reopen catalog");
    assert_eq!(catalog.schema_version().expect("schema version"), 4);
    assert_eq!(catalog.list_services().expect("list services").len(), 1);
}

#[test]
fn catalog_rejects_duplicate_service_slug_in_environment() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut catalog =
        Catalog::open(directory.path().join("catalog.sqlite3")).expect("open catalog");
    let organization_id = catalog
        .create_organization("default", "Default")
        .expect("create organization");
    let project_id = catalog
        .create_project(&organization_id, "hakobi", "Hakobi")
        .expect("create project");
    let environment_id = catalog
        .create_environment(&project_id, "production", "Production")
        .expect("create environment");
    catalog
        .create_service(&environment_id, "web", "Web")
        .expect("first service");
    assert!(catalog
        .create_service(&environment_id, "web", "Duplicate")
        .is_err());
}
#[test]
fn host_agents_unique_name_and_url() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).expect("open catalog");

    catalog
        .create_host_agent("alpha", "https://alpha.example.com", "tok_0123456789abcdef")
        .expect("create alpha");
    catalog
        .create_host_agent("beta", "https://beta.example.com", "tok_0123456789abcdef")
        .expect("create beta");

    assert!(catalog
        .create_host_agent("alpha", "https://other.example.com", "tok_0123456789abcdef")
        .is_err());
    assert!(catalog
        .create_host_agent("gamma", "https://alpha.example.com", "tok_0123456789abcdef")
        .is_err());
}

#[test]
fn host_agent_health_is_recorded() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).expect("open catalog");

    let host_id = catalog
        .create_host_agent("health-host", "https://health.example.com", "tok_health_01")
        .expect("create host");

    catalog
        .record_host_agent_health(&host_id, None)
        .expect("record ok");
    catalog
        .record_host_agent_health(&host_id, Some("connection timed out"))
        .expect("record error");

    let agents = catalog.list_host_agents().expect("list");
    let agent = agents.iter().find(|a| a.id == host_id).unwrap();
    assert!(agent.last_checked_at.is_some());
    assert_eq!(agent.last_error.as_deref(), Some("connection timed out"));
}

#[test]
fn host_agent_toggle_and_delete() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).expect("open catalog");

    let host_id = catalog
        .create_host_agent("toggle-host", "https://toggle.example.com", "tok_toggle_01")
        .expect("create host");

    catalog
        .set_host_agent_enabled(&host_id, false)
        .expect("disable");
    let agents = catalog.list_host_agents().expect("list");
    assert!(!agents[0].enabled);

    catalog
        .set_host_agent_enabled(&host_id, true)
        .expect("enable");
    let agents = catalog.list_host_agents().expect("list");
    assert!(agents[0].enabled);

    catalog.delete_host_agent(&host_id).expect("delete");
    let agents = catalog.list_host_agents().expect("list");
    assert!(agents.is_empty());

    // Delete non-existent should error.
    assert!(catalog.delete_host_agent("nonexistent").is_err());
}

#[test]
fn host_agent_provider_creation_is_atomic() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).expect("open catalog");

    let host_id = catalog
        .create_host_agent("atomic-host", "https://atomic.example.com", "tok_atomic_01")
        .expect("create host");

    let provider_id = catalog
        .create_host_agent_provider(&host_id)
        .expect("create provider");
    assert!(!provider_id.is_empty());

    let providers = catalog.list_provider_connections().expect("list providers");
    let provider = providers.iter().find(|p| p.id == provider_id).unwrap();
    assert_eq!(provider.kind, "docker");
    assert_eq!(provider.endpoint, format!("agent://{host_id}"));
    assert!(provider.enabled);

    // Second call should return existing provider ID.
    let provider_id_2 = catalog
        .create_host_agent_provider(&host_id)
        .expect("create again");
    assert_eq!(provider_id, provider_id_2);

    catalog
        .delete_host_agent_provider(&host_id)
        .expect("delete provider");
    let providers = catalog.list_provider_connections().expect("list");
    assert!(!providers
        .iter()
        .any(|p| p.endpoint == format!("agent://{host_id}")));
}

#[test]
fn host_agents_are_not_exposed_in_provider_connection() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).expect("open catalog");

    let host_id = catalog
        .create_host_agent(
            "secret-agent",
            "https://secret.example.com",
            "tok_secret_01",
        )
        .expect("create host");

    let providers = catalog.list_provider_connections().expect("list providers");
    for provider in &providers {
        assert!(!provider.name.contains("secret"));
        assert!(!provider.endpoint.contains("tok_"));
    }

    // Token should not leak into provider connection view.
    let host = catalog.host_agent_token(&host_id).expect("read token");
    assert_eq!(host, "tok_secret_01");

    let agents = catalog.list_host_agents().expect("list");
    let agent = agents.iter().find(|a| a.id == host_id).unwrap();
    assert!(!agent.agent_url.contains("tok_secret_01"));
}
