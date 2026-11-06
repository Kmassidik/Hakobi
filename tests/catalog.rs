use hakobi::catalog::Catalog;

#[test]
fn catalog_persists_hierarchy_across_reopen() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");

    let service_id;
    {
        let mut catalog = Catalog::open(&path).expect("open catalog");
        assert_eq!(catalog.schema_version().expect("schema version"), 3);

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
    assert_eq!(catalog.schema_version().expect("schema version"), 3);
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
