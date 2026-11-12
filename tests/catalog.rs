use hakobi::catalog::Catalog;

#[test]
fn catalog_persists_apps_across_reopen() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");

    let app_id;
    {
        let mut catalog = Catalog::open(&path).expect("open catalog");
        assert_eq!(catalog.schema_version().expect("schema version"), 9);
        let host_id = catalog
            .create_host_agent("apps-host", "https://apps.example.com", "tok_apps_0123456")
            .expect("create host");

        app_id = catalog
            .create_app(
                "web",
                &host_id,
                "dockerfile",
                Some("Kmassidik/web"),
                Some("main"),
                None,
                Some("Dockerfile"),
                true,
                Some(3000),
            )
            .expect("create app");

        let apps = catalog.list_apps().expect("list apps");
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, app_id);
        assert_eq!(apps[0].name, "web");
        assert_eq!(apps[0].github_repo.as_deref(), Some("Kmassidik/web"));
        assert_eq!(apps[0].github_branch.as_deref(), Some("main"));
        assert!(apps[0].github_auto_deploy);
        assert_eq!(apps[0].container_port, Some(3000));
        assert_eq!(apps[0].status, "idle");
        assert!(!apps[0].created_at.is_empty());
    }

    let catalog = Catalog::open(&path).expect("reopen catalog");
    assert_eq!(catalog.schema_version().expect("schema version"), 9);
    assert_eq!(catalog.list_apps().expect("list apps").len(), 1);
}

#[test]
fn apps_crud_round_trip() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");

    let app_id;
    {
        let mut catalog = Catalog::open(&path).expect("open catalog");
        assert_eq!(catalog.schema_version().expect("schema version"), 9);
        let host_id = catalog
            .create_host_agent("crud-host", "https://crud.example.com", "tok_crud_01234567")
            .expect("create host");

        app_id = catalog
            .create_app(
                "web-prod",
                &host_id,
                "dockerfile",
                Some("user/repo"),
                Some("main"),
                None,
                Some("Dockerfile"),
                false,
                Some(8080),
            )
            .expect("create app");

        let loaded = catalog
            .get_app(&app_id)
            .expect("get app")
            .expect("app exists");
        assert_eq!(loaded.name, "web-prod");
        assert_eq!(loaded.build_type, "dockerfile");
        assert_eq!(loaded.container_port, Some(8080));
        assert!(catalog
            .get_app("missing-id")
            .expect("get missing app")
            .is_none());

        catalog
            .update_app(
                &app_id,
                "web-prod-2",
                &host_id,
                "nixpacks",
                Some("user/other"),
                Some("develop"),
                Some("services/api"),
                Some("docker/Dockerfile"),
                true,
                None,
            )
            .expect("update app");
        let updated = catalog
            .get_app(&app_id)
            .expect("get app")
            .expect("app exists");
        assert_eq!(updated.name, "web-prod-2");
        assert_eq!(updated.build_type, "nixpacks");
        assert_eq!(updated.github_branch.as_deref(), Some("develop"));
        assert_eq!(updated.github_path.as_deref(), Some("services/api"));
        assert!(updated.github_auto_deploy);
        assert_eq!(updated.container_port, None);

        catalog.delete_app(&app_id).expect("delete app");
        assert!(catalog.list_apps().expect("list apps").is_empty());
    }
}

#[test]
fn apps_validate_name_host_and_enums() {
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
        ($name:expr, $host:expr, $build:expr, $port:expr) => {
            catalog.create_app(
                $name,
                $host,
                $build,
                Some("user/repo"),
                Some("main"),
                None,
                Some("Dockerfile"),
                false,
                $port,
            )
        };
    }

    // unique ASCII slug, 1-64 chars
    assert!(create!("", &host_id, "dockerfile", Some(3000)).is_err());
    let long_name = "a".repeat(65);
    assert!(create!(long_name.as_str(), &host_id, "dockerfile", Some(3000)).is_err());
    assert!(create!("bad name", &host_id, "dockerfile", Some(3000)).is_err());
    assert!(create!("bad/name", &host_id, "dockerfile", Some(3000)).is_err());

    // host must exist and be enabled
    assert!(create!("app-a", "no-such-host", "dockerfile", Some(3000)).is_err());
    assert!(create!("app-b", &disabled_id, "dockerfile", Some(3000)).is_err());

    // build enum
    assert!(create!("app-c", &host_id, "helm", Some(3000)).is_err());
    let first = create!("app-d", &host_id, "dockerfile", Some(3000)).expect("create app-d");

    // duplicate name rejected
    assert!(create!("app-d", &host_id, "dockerfile", Some(3000)).is_err());

    // port bounds
    assert!(catalog
        .update_app(
            &first,
            "app-d",
            &host_id,
            "dockerfile",
            Some("user/repo"),
            Some("main"),
            None,
            Some("Dockerfile"),
            false,
            Some(0)
        )
        .is_err());

    // update validates the same way
    assert!(catalog
        .update_app(
            "missing",
            "x",
            &host_id,
            "dockerfile",
            None,
            None,
            None,
            None,
            false,
            None
        )
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
    let app_id = catalog
        .create_app(
            "api",
            &host_id,
            "dockerfile",
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .expect("create app");

    assert!(catalog
        .create_deployment("missing-app", "manual", None, None)
        .is_err());
    assert!(catalog
        .create_deployment(&app_id, "cron", None, None)
        .is_err());

    let first = catalog
        .create_deployment(&app_id, "manual", None, None)
        .expect("create deployment");
    let second = catalog
        .create_deployment(&app_id, "webhook", Some("abc123"), None)
        .expect("create second deployment");
    assert_ne!(first, second);

    let deployments = catalog.list_deployments(&app_id).expect("list deployments");
    assert_eq!(deployments.len(), 2);
    assert_eq!(deployments[0].id, second);
    assert_eq!(deployments[0].version, 2);
    assert_eq!(deployments[0].trigger_type, "webhook");
    assert_eq!(deployments[0].git_commit_sha.as_deref(), Some("abc123"));
    assert_eq!(deployments[1].id, first);
    assert_eq!(deployments[1].version, 1);
    assert_eq!(deployments[1].status, "pending");

    let latest = catalog
        .get_latest_deployment(&app_id)
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
        .list_deployments(&app_id)
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

    // cancelled status accepted (newest-wins semantics)
    catalog
        .update_deployment_status(&second, "cancelled", None)
        .expect("cancel deployment");
}
