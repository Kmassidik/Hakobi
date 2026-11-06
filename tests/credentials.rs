use std::fs;

use hakobi::catalog::Catalog;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn provider_credentials_are_encrypted_rotated_and_revoked() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).expect("open catalog");
    assert_eq!(catalog.schema_version().expect("schema version"), 3);

    let connection_id = catalog
        .create_provider_connection(
            "docker-local",
            "docker",
            "unix:///var/run/docker.sock",
            "super-secret-token",
        )
        .expect("create provider connection");

    let connections = catalog
        .list_provider_connections()
        .expect("list provider connections");
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, connection_id);
    assert_eq!(connections[0].name, "docker-local");
    assert_eq!(connections[0].kind, "docker");
    assert_eq!(connections[0].endpoint, "unix:///var/run/docker.sock");
    assert!(connections[0].enabled);
    assert!(connections[0].capabilities.is_empty());
    assert!(connections[0].last_checked_at.is_none());
    assert!(connections[0].last_error.is_none());
    assert!(!format!("{connections:?}").contains("super-secret-token"));

    assert_eq!(
        catalog
            .provider_secret(&connection_id)
            .expect("read secret"),
        "super-secret-token"
    );
    assert!(!contains(
        &fs::read(&path).expect("read database"),
        b"super-secret-token"
    ));
    assert!(path.with_extension("key").is_file());

    catalog
        .rotate_provider_secret(&connection_id, "rotated-token")
        .expect("rotate secret");
    assert_eq!(
        catalog
            .provider_secret(&connection_id)
            .expect("read rotated secret"),
        "rotated-token"
    );
    assert!(!contains(
        &fs::read(&path).expect("read database"),
        b"rotated-token"
    ));

    catalog
        .set_provider_capabilities(&connection_id, &["read", "operate"])
        .expect("set capabilities");
    let connections = catalog
        .list_provider_connections()
        .expect("list provider connections");
    assert_eq!(connections[0].capabilities, vec!["read", "operate"]);
    assert!(catalog
        .set_provider_capabilities(&connection_id, &["root"])
        .is_err());

    catalog
        .record_provider_health(&connection_id, Some("connection refused"))
        .expect("record failed health");
    let connections = catalog
        .list_provider_connections()
        .expect("list provider connections");
    assert!(connections[0].last_checked_at.is_some());
    assert_eq!(
        connections[0].last_error.as_deref(),
        Some("connection refused")
    );

    catalog
        .record_provider_health(&connection_id, None)
        .expect("record healthy state");
    let connections = catalog
        .list_provider_connections()
        .expect("list provider connections");
    assert!(connections[0].last_error.is_none());

    catalog
        .revoke_provider_connection(&connection_id)
        .expect("revoke provider connection");
    assert!(catalog.provider_secret(&connection_id).is_err());
    let connections = catalog
        .list_provider_connections()
        .expect("list provider connections");
    assert!(!connections[0].enabled);

    let audit = catalog.list_audit_events().expect("audit events");
    assert!(audit.iter().any(|event| {
        event.action == "rotate"
            && event.entity_kind == "provider_connection"
            && event.entity_id == connection_id
    }));
    assert!(audit.iter().any(|event| {
        event.action == "revoke"
            && event.entity_kind == "provider_connection"
            && event.entity_id == connection_id
    }));
    let audit_text = format!("{audit:?}");
    assert!(!audit_text.contains("super-secret-token"));
    assert!(!audit_text.contains("rotated-token"));
}
