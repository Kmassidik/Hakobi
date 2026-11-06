# Hakobi Checkpoint 1: encrypted provider credentials

Extend the existing `src/catalog.rs` SQLite catalog with provider connections, encrypted credentials, capability flags, freshness, and audit events. The acceptance test in `tests/credentials.rs` is the contract and must pass unchanged.

## Required API

Add publicly:

```rust
#[derive(Debug, Clone)]
pub struct ProviderConnection {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub enabled: bool,
    pub last_checked_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

impl Catalog {
    pub fn create_provider_connection(
        &mut self,
        name: &str,
        kind: &str,
        endpoint: &str,
        secret: &str,
    ) -> anyhow::Result<String>;
    pub fn list_provider_connections(&self) -> anyhow::Result<Vec<ProviderConnection>>;
    pub fn provider_secret(&self, id: &str) -> anyhow::Result<String>;
    pub fn rotate_provider_secret(&mut self, id: &str, secret: &str) -> anyhow::Result<()>;
    pub fn revoke_provider_connection(&mut self, id: &str) -> anyhow::Result<()>;
    pub fn set_provider_capabilities(
        &mut self,
        id: &str,
        capabilities: &[&str],
    ) -> anyhow::Result<()>;
    pub fn record_provider_health(
        &mut self,
        id: &str,
        error: Option<&str>,
    ) -> anyhow::Result<()>;
}
```

## Requirements

- Bump the catalog schema to version 2. Existing version-1 databases must migrate in place. Existing catalog behavior and audit immutability must remain.
- Create a `provider_connections` table with a unique name, kind, endpoint, encrypted credential, capability list, enabled flag, freshness fields, and creation timestamp.
- Encrypt secrets at rest with ChaCha20-Poly1305 (`chacha20poly1305` crate) using a random 32-byte key stored in a sidecar file next to the database: `catalog.sqlite3` uses `catalog.key`. Create the key with Unix mode `0600` if missing. Never store the plaintext secret in the database, logs, or audit rows.
- Store a fresh random nonce for every encryption. Encode nonce+ciphertext as base64 in SQLite.
- `list_provider_connections` must not expose secrets. `provider_secret` decrypts only server-side.
- Capabilities are restricted to `read`, `operate`, and `admin`; reject unknown values.
- `revoke_provider_connection` disables the connection and removes its encrypted secret; subsequent `provider_secret` must fail.
- `record_provider_health` sets `last_checked_at`; a `Some(error)` sets `last_error`, `None` clears it.
- Audit `create`, `rotate`, `revoke`, and capability changes. Audit events must not include secrets.
- Use parameterized SQL only.
- Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt`.

## Fences

- Edit only `src/catalog.rs`, `Cargo.toml`, and `Cargo.lock`.
- Do not edit `tests/credentials.rs`, `tests/catalog.rs`, or any other file.
- Do not access the network or secrets.
