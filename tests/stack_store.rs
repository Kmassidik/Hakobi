use hakobi::catalog::Catalog;
use hakobi::stacks::StackKind;
use rusqlite::{params, Connection};

const COMPOSE: &str = "\
x-defaults: &defaults
  restart: unless-stopped
services:
  web:
    <<: *defaults
    image: nginx:latest
    ports:
      - \"8080:80\"
";

const KUBERNETES: &str = "\
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
  labels:
    app: web
spec:
  replicas: 2
  template:
    spec:
      containers:
      - name: web
        image: nginx:latest
        ports:
        - containerPort: 80
";

const DOCKERFILE: &str = "\
FROM rust:1.88 AS build
WORKDIR /app
RUN echo 'hello: [world'
";

fn open_catalog() -> (tempfile::TempDir, std::path::PathBuf, Catalog) {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");
    let catalog = Catalog::open(&path).expect("open catalog");
    (directory, path, catalog)
}

#[test]
fn stack_definitions_round_trip_through_the_catalog() {
    let (_directory, path, mut catalog) = open_catalog();
    assert_eq!(catalog.schema_version().expect("schema version"), 7);

    let provider_id = catalog
        .create_provider_connection(
            "docker-local",
            "docker",
            "unix:///var/run/docker.sock",
            "secret",
        )
        .expect("create provider");
    assert!(catalog.list_stacks().expect("list stacks").is_empty());

    let stack_id = catalog
        .create_stack(
            &provider_id,
            "web",
            StackKind::Compose,
            "git@github.com:acme/web.git",
            COMPOSE,
        )
        .expect("create stack");

    let stacks = catalog.list_stacks().expect("list stacks");
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].id, stack_id);
    assert_eq!(stacks[0].provider_id, provider_id);
    assert_eq!(stacks[0].name, "web");
    assert_eq!(stacks[0].kind, StackKind::Compose);
    assert_eq!(stacks[0].source, "git@github.com:acme/web.git");
    assert_eq!(stacks[0].yaml, COMPOSE);
    assert!(!stacks[0].created_at.is_empty());
    assert!(!stacks[0].updated_at.is_empty());

    assert!(catalog.get_stack("missing").expect("get missing").is_none());
    let stored = catalog
        .get_stack(&stack_id)
        .expect("get stack")
        .expect("stack exists");
    assert_eq!(stored.name, "web");
    assert_eq!(stored.created_at, stacks[0].created_at);

    catalog
        .update_stack(
            &stack_id,
            "web-api",
            StackKind::Kubernetes,
            "https://github.com/acme/web",
            KUBERNETES,
        )
        .expect("update stack");

    let updated = catalog
        .get_stack(&stack_id)
        .expect("get stack")
        .expect("stack exists");
    assert_eq!(updated.name, "web-api");
    assert_eq!(updated.kind, StackKind::Kubernetes);
    assert_eq!(updated.source, "https://github.com/acme/web");
    assert_eq!(updated.yaml, KUBERNETES);
    assert_eq!(updated.provider_id, provider_id);
    assert_eq!(updated.created_at, stacks[0].created_at);

    drop(catalog);

    let mut catalog = Catalog::open(&path).expect("reopen catalog");
    assert_eq!(catalog.schema_version().expect("schema version"), 7);
    assert_eq!(
        catalog
            .get_stack(&stack_id)
            .expect("get stack")
            .expect("stack persists")
            .name,
        "web-api"
    );

    catalog.delete_stack(&stack_id).expect("delete stack");
    assert!(catalog.get_stack(&stack_id).expect("get stack").is_none());
    assert!(catalog.list_stacks().expect("list stacks").is_empty());
    assert!(catalog.delete_stack(&stack_id).is_err());

    let audit = catalog.list_audit_events().expect("list audit events");
    let stack_events: Vec<_> = audit
        .iter()
        .filter(|event| event.entity_kind == "stack_definition" && event.entity_id == stack_id)
        .collect();
    for action in ["create", "update", "delete"] {
        assert!(
            stack_events
                .iter()
                .any(|event| event.action == action && event.actor == "system"),
            "missing {action} audit event"
        );
    }
}

#[test]
fn stack_definitions_reject_duplicate_names_and_missing_providers() {
    let (_directory, _path, mut catalog) = open_catalog();
    let provider_id = catalog
        .create_provider_connection("docker-a", "docker", "unix:///a.sock", "a")
        .expect("create provider a");
    let other_provider_id = catalog
        .create_provider_connection("docker-b", "docker", "unix:///b.sock", "b")
        .expect("create provider b");

    assert!(catalog
        .create_stack(
            "missing-provider",
            "api",
            StackKind::Compose,
            "source",
            COMPOSE
        )
        .is_err());

    catalog
        .create_stack(&provider_id, "api", StackKind::Compose, "source", COMPOSE)
        .expect("create api");
    let worker_id = catalog
        .create_stack(
            &provider_id,
            "worker",
            StackKind::Compose,
            "source",
            COMPOSE,
        )
        .expect("create worker");

    assert!(catalog
        .create_stack(&provider_id, "api", StackKind::Compose, "other", COMPOSE)
        .is_err());

    // The same name is allowed on a different provider.
    catalog
        .create_stack(
            &other_provider_id,
            "api",
            StackKind::Compose,
            "source",
            COMPOSE,
        )
        .expect("duplicate name on another provider");

    // Updating onto an existing name in the same provider is rejected.
    assert!(catalog
        .update_stack(&worker_id, "api", StackKind::Compose, "source", COMPOSE)
        .is_err());
    assert_eq!(
        catalog
            .get_stack(&worker_id)
            .expect("get worker")
            .expect("worker exists")
            .name,
        "worker"
    );

    // Updating a stack without renaming it is allowed.
    catalog
        .update_stack(
            &worker_id,
            "worker",
            StackKind::Kubernetes,
            "updated",
            KUBERNETES,
        )
        .expect("update worker");
    assert!(catalog
        .update_stack(
            "missing-stack",
            "worker",
            StackKind::Compose,
            "source",
            COMPOSE
        )
        .is_err());
}

#[test]
fn stack_definitions_reject_invalid_yaml_and_accept_build_descriptions() {
    let (_directory, _path, mut catalog) = open_catalog();
    let provider_id = catalog
        .create_provider_connection("docker-local", "docker", "unix:///docker.sock", "secret")
        .expect("create provider");

    let invalid = [
        (StackKind::Compose, "services: [web\n"),
        (
            StackKind::Compose,
            "services:\n  web:\n    image: nginx\n   broken: indent\n",
        ),
        (StackKind::Compose, "image: \"unterminated\n"),
        (StackKind::Kubernetes, "metadata: {name: web\n"),
        (StackKind::Kubernetes, "\tapiVersion: v1\n"),
        (StackKind::Compose, ""),
    ];
    for (kind, yaml) in invalid {
        assert!(
            catalog
                .create_stack(&provider_id, "invalid", kind.clone(), "source", yaml)
                .is_err(),
            "expected {kind} document to be rejected: {yaml:?}"
        );
    }
    assert!(catalog.list_stacks().expect("list stacks").is_empty());

    let dockerfile_id = catalog
        .create_stack(
            &provider_id,
            "builder",
            StackKind::Dockerfile,
            ".",
            DOCKERFILE,
        )
        .expect("create dockerfile stack");
    catalog
        .create_stack(
            &provider_id,
            "nixpacks",
            StackKind::Nixpacks,
            ".",
            "build = \"cargo build --release\"\n",
        )
        .expect("create nixpacks stack");

    assert!(catalog
        .update_stack(
            &dockerfile_id,
            "builder",
            StackKind::Compose,
            ".",
            "services: {web: [nginx}\n"
        )
        .is_err());
    let unchanged = catalog
        .get_stack(&dockerfile_id)
        .expect("get stack")
        .expect("stack exists");
    assert_eq!(unchanged.kind, StackKind::Dockerfile);
    assert_eq!(unchanged.yaml, DOCKERFILE);

    let audit = catalog.list_audit_events().expect("list audit events");
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.entity_kind == "stack_definition")
            .count(),
        2,
        "rejected writes must not be audited"
    );
}

const V2_SCHEMA: &str = "\
CREATE TABLE provider_connections (
    id               TEXT PRIMARY KEY NOT NULL,
    name             TEXT NOT NULL UNIQUE,
    kind             TEXT NOT NULL,
    endpoint         TEXT NOT NULL,
    encrypted_secret TEXT,
    capabilities     TEXT NOT NULL DEFAULT '[]',
    enabled          INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_checked_at  TEXT,
    last_error       TEXT,
    created_at       TEXT NOT NULL CHECK (length(created_at) > 0)
);

CREATE TABLE audit_events (
    id          TEXT PRIMARY KEY NOT NULL,
    action      TEXT NOT NULL,
    entity_kind TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    actor       TEXT NOT NULL,
    created_at  TEXT NOT NULL CHECK (length(created_at) > 0)
);

PRAGMA user_version = 2;
";

#[test]
fn catalog_migrates_v2_database_and_preserves_providers() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("catalog.sqlite3");

    {
        let connection = Connection::open(&path).expect("open legacy database");
        connection
            .execute_batch(V2_SCHEMA)
            .expect("create v2 schema");
        connection
            .execute(
                "INSERT INTO provider_connections \
                     (id, name, kind, endpoint, capabilities, enabled, created_at) \
                 VALUES (?1, ?2, ?3, ?4, '[]', 1, ?5)",
                params![
                    "legacy-provider",
                    "legacy",
                    "docker",
                    "unix:///var/run/docker.sock",
                    "2024-01-01T00:00:00.000Z"
                ],
            )
            .expect("insert legacy provider");
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .expect("read legacy version"),
            2
        );
    }

    let mut catalog = Catalog::open(&path).expect("migrate catalog");
    assert_eq!(catalog.schema_version().expect("schema version"), 7);

    let providers = catalog.list_provider_connections().expect("list providers");
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].id, "legacy-provider");
    assert_eq!(providers[0].name, "legacy");
    assert!(catalog.list_stacks().expect("list stacks").is_empty());

    let stack_id = catalog
        .create_stack(
            "legacy-provider",
            "migrated",
            StackKind::Compose,
            "source",
            COMPOSE,
        )
        .expect("create stack after migration");
    assert_eq!(
        catalog
            .get_stack(&stack_id)
            .expect("get stack")
            .expect("stack exists")
            .name,
        "migrated"
    );
}
