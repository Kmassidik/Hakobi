use hakobi::catalog::Catalog;

#[test]
fn catalog_persists_hierarchy_across_reopen() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");

    let service_id;
    {
        let mut catalog = Catalog::open(&path).expect("open catalog");
        assert_eq!(catalog.schema_version().expect("schema version"), 5);

        let organization_id = catalog
            .create_organization("default", "Default")
            .expect("create organization");
        let project_id = catalog
            .create_project(&organization_id, "hakobi", "Hakobi")
            .expect("create project");
        let environment_id = catalog
            .create_project_environment(&project_id, "production", "Production")
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
    assert_eq!(catalog.schema_version().expect("schema version"), 5);
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
        .create_project_environment(&project_id, "production", "Production")
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

#[test]
fn environments_crud_round_trip() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");

    let environment_id;
    {
        let mut catalog = Catalog::open(&path).expect("open catalog");
        assert_eq!(catalog.schema_version().expect("schema version"), 5);
        let host_id = catalog
            .create_host_agent("env-host", "https://env.example.com", "tok_env_0123456789")
            .expect("create host");

        environment_id = catalog
            .create_environment(
                "web-prod",
                &host_id,
                "dockerfile",
                "git",
                Some("https://github.com/user/repo.git"),
                Some("main"),
                "local",
            )
            .expect("create environment");

        let environments = catalog.list_environments().expect("list environments");
        assert_eq!(environments.len(), 1);
        let environment = &environments[0];
        assert_eq!(environment.id, environment_id);
        assert_eq!(environment.name, "web-prod");
        assert_eq!(environment.host_id, host_id);
        assert_eq!(environment.build_type, "dockerfile");
        assert_eq!(environment.source_type, "git");
        assert_eq!(environment.status, "idle");
        assert_eq!(environment.registry_mode, "local");
        assert!(!environment.created_at.is_empty());
        assert!(!environment.updated_at.is_empty());

        let loaded = catalog
            .get_environment(&environment_id)
            .expect("get environment")
            .expect("environment exists");
        assert_eq!(loaded.name, "web-prod");
        assert!(catalog
            .get_environment("missing-id")
            .expect("get missing environment")
            .is_none());

        catalog
            .update_environment(
                &environment_id,
                "web-prod-2",
                &host_id,
                "compose",
                "upload",
                None,
                None,
                "push",
            )
            .expect("update environment");
        let updated = catalog
            .get_environment(&environment_id)
            .expect("get environment")
            .expect("environment exists");
        assert_eq!(updated.name, "web-prod-2");
        assert_eq!(updated.build_type, "compose");
        assert_eq!(updated.source_type, "upload");
        assert_eq!(updated.registry_mode, "push");
        assert!(updated.git_url.is_none());

        let audit = catalog.list_audit_events().expect("list audit events");
        assert!(audit
            .iter()
            .any(|event| event.action == "create" && event.entity_kind == "environment"));
        assert!(audit
            .iter()
            .any(|event| event.action == "update" && event.entity_kind == "environment"));
    }

    let mut catalog = Catalog::open(&path).expect("reopen catalog");
    assert_eq!(
        catalog
            .list_environments()
            .expect("list environments")
            .len(),
        1
    );
    catalog
        .delete_environment(&environment_id)
        .expect("delete environment");
    assert!(catalog
        .list_environments()
        .expect("list environments")
        .is_empty());
    assert!(catalog.delete_environment(&environment_id).is_err());
    let audit = catalog.list_audit_events().expect("list audit events");
    assert!(audit
        .iter()
        .any(|event| event.action == "delete" && event.entity_kind == "environment"));
}

#[test]
fn environments_validate_name_host_and_enums() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut catalog =
        Catalog::open(directory.path().join("catalog.sqlite3")).expect("open catalog");
    let host_id = catalog
        .create_host_agent(
            "valid-host",
            "https://valid.example.com",
            "tok_valid_0123456",
        )
        .expect("create host");
    let disabled_id = catalog
        .create_host_agent("off-host", "https://off.example.com", "tok_off_012345678")
        .expect("create host");
    catalog
        .set_host_agent_enabled(&disabled_id, false)
        .expect("disable host");

    macro_rules! create {
        ($name:expr, $host:expr, $build:expr, $source:expr, $url:expr) => {
            catalog.create_environment($name, $host, $build, $source, $url, Some("main"), "local")
        };
    }

    // unique ASCII slug, 1-64 chars
    assert!(create!("", &host_id, "compose", "upload", None).is_err());
    let long_name = "a".repeat(65);
    assert!(create!(long_name.as_str(), &host_id, "compose", "upload", None).is_err());
    assert!(create!("bad name", &host_id, "compose", "upload", None).is_err());
    assert!(create!("bad/name", &host_id, "compose", "upload", None).is_err());

    // host must exist and be enabled
    assert!(create!("env-a", "no-such-host", "compose", "upload", None).is_err());
    assert!(create!("env-b", &disabled_id, "compose", "upload", None).is_err());

    // build/source/registry enums
    assert!(create!("env-c", &host_id, "helm", "upload", None).is_err());
    assert!(create!("env-d", &host_id, "compose", "svn", None).is_err());
    assert!(create!("env-e", &host_id, "compose", "upload", None)
        .map(|_| ())
        .is_ok());

    // git/upload consistency
    assert!(create!("env-f", &host_id, "compose", "git", None).is_err());
    assert!(create!("env-g", &host_id, "compose", "git", Some("  ")).is_err());
    assert!(create!(
        "env-h",
        &host_id,
        "compose",
        "upload",
        Some("https://x.git")
    )
    .is_err());

    // duplicate name rejected
    assert!(create!("env-e", &host_id, "compose", "upload", None).is_err());

    // update validates the same way
    assert!(catalog
        .update_environment("missing", "x", &host_id, "compose", "upload", None, None, "local")
        .is_err());
}

#[test]
fn deployments_version_status_and_logs() {
    let directory = tempfile::tempdir().expect("tempdir");
    let mut catalog =
        Catalog::open(directory.path().join("catalog.sqlite3")).expect("open catalog");
    let host_id = catalog
        .create_host_agent(
            "deploy-host",
            "https://deploy.example.com",
            "tok_deploy_012345",
        )
        .expect("create host");
    let environment_id = catalog
        .create_environment("api", &host_id, "compose", "upload", None, None, "local")
        .expect("create environment");

    assert!(catalog
        .create_deployment("missing-env", "manual", None, None)
        .is_err());
    assert!(catalog
        .create_deployment(&environment_id, "cron", None, None)
        .is_err());

    let first = catalog
        .create_deployment(&environment_id, "manual", None, None)
        .expect("create deployment");
    let second = catalog
        .create_deployment(&environment_id, "webhook", Some("abc123"), None)
        .expect("create second deployment");
    assert_ne!(first, second);

    let deployments = catalog
        .list_deployments(&environment_id)
        .expect("list deployments");
    assert_eq!(deployments.len(), 2);
    assert_eq!(deployments[0].id, second);
    assert_eq!(deployments[0].version, 2);
    assert_eq!(deployments[0].trigger_type, "webhook");
    assert_eq!(deployments[0].git_commit_sha.as_deref(), Some("abc123"));
    assert_eq!(deployments[1].id, first);
    assert_eq!(deployments[1].version, 1);
    assert_eq!(deployments[1].status, "pending");

    let latest = catalog
        .get_latest_deployment(&environment_id)
        .expect("latest deployment")
        .expect("latest exists");
    assert_eq!(latest.id, second);

    catalog
        .update_deployment_status(&first, "building", Some("build output"))
        .expect("update status");
    catalog
        .update_deployment_status(&first, "failed", None)
        .expect("fail deployment");
    assert!(catalog
        .update_deployment_status(&first, "not-a-status", None)
        .is_err());
    assert!(catalog
        .update_deployment_status("missing", "running", None)
        .is_err());

    let failed = catalog
        .list_deployments(&environment_id)
        .expect("list deployments")
        .into_iter()
        .find(|deployment| deployment.id == first)
        .expect("deployment exists");
    assert_eq!(failed.status, "failed");
    assert!(failed.completed_at.is_some());
    assert_eq!(failed.build_output.as_deref(), Some("build output"));

    catalog
        .add_deployment_log(&first, "info", "starting build")
        .expect("add log");
    catalog
        .add_deployment_log(&first, "error", "build blew up")
        .expect("add error log");
    assert!(catalog.add_deployment_log(&first, "debug", "nope").is_err());
    assert!(catalog.add_deployment_log("missing", "info", "x").is_err());

    let logs = catalog.list_deployment_logs(&first).expect("list logs");
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0].level, "info");
    assert_eq!(logs[0].message, "starting build");
    assert_eq!(logs[1].level, "error");
    assert!(!logs[0].timestamp.is_empty());

    // audit events recorded for deployment create
    let audit = catalog.list_audit_events().expect("list audit events");
    assert!(
        audit
            .iter()
            .filter(|event| event.entity_kind == "deployment")
            .count()
            >= 2
    );
}
