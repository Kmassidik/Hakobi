# Hakobi Checkpoint 1: persistent catalog foundation

Implement the first persistent data foundation for Hakobi. The current application is a Rust binary with secure setup and SSR; it has no library target or database.

## Required API

Create `src/lib.rs` with:

```rust
pub mod catalog;
```

Create `src/catalog.rs` and publicly expose:

```rust
pub struct Catalog { /* private */ }

#[derive(Debug, Clone)]
pub struct Service {
    pub id: String,
    pub environment_id: String,
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub id: String,
    pub action: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub actor: String,
    pub created_at: String,
}

impl Catalog {
    pub fn open(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self>;
    pub fn schema_version(&self) -> anyhow::Result<i64>;
    pub fn create_organization(&mut self, slug: &str, name: &str) -> anyhow::Result<String>;
    pub fn create_project(&mut self, organization_id: &str, slug: &str, name: &str) -> anyhow::Result<String>;
    pub fn create_environment(&mut self, project_id: &str, slug: &str, name: &str) -> anyhow::Result<String>;
    pub fn create_service(&mut self, environment_id: &str, slug: &str, name: &str) -> anyhow::Result<String>;
    pub fn list_services(&self) -> anyhow::Result<Vec<Service>>;
    pub fn list_audit_events(&self) -> anyhow::Result<Vec<AuditEvent>>;
}
```

Use SQLite with `rusqlite` (bundled SQLite is acceptable) and add any needed dependencies to `Cargo.toml`. Do not edit `tests/catalog.rs`; it is the independent acceptance test.

## Requirements

- Enable foreign keys.
- Create tables for organizations, projects, environments, services, and immutable audit events.
- Set `PRAGMA user_version = 1` and make reopening/migration idempotent.
- IDs must be stable strings (UUID is fine).
- Enforce unique `(environment_id, slug)` for services.
- Every create operation writes an audit event with action `create`, the entity kind, the created entity ID, and actor `system`.
- Timestamps must be non-empty strings stored in the database.
- Use parameterized SQL only. Do not interpolate user values into SQL.
- Keep the existing binary and its 11 tests working.
- Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`.

## Constraints

- Edit only `src/lib.rs`, `src/catalog.rs`, `Cargo.toml`, `Cargo.lock`, and optionally `migrations/**`.
- Do not edit `tests/catalog.rs`, existing source modules, or tests.
- Do not access the network or secrets.
- Your claim of success is not evidence; the harness runs the checks.
