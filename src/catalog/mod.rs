//! Persistent catalog for providers, hosts, stacks, environments, and
//! deployments.
//!
//! The catalog is a SQLite database. Its schema is versioned with
//! `PRAGMA user_version`; opening an up-to-date database is a no-op.

mod environments;
mod hosts;
mod legacy;
mod providers;
mod stacks;

pub use environments::{
    validate_build_type, validate_environment_name, validate_registry_mode, validate_source_type,
    Deployment, DeploymentLog, Environment,
};
pub use hosts::HostAgent;
pub use legacy::{AuditEvent, Service};
pub use providers::ProviderConnection;
pub use stacks::StackDefinition;

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use rusqlite::{params, Connection, Transaction};
use uuid::Uuid;

/// Schema version written to `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 5;

/// Capabilities a provider connection may be granted.
const CAPABILITIES: [&str; 3] = ["read", "operate", "admin"];

/// Length of a ChaCha20-Poly1305 key and nonce in bytes.
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS organizations (
    id         TEXT PRIMARY KEY NOT NULL,
    slug       TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0)
);

CREATE TABLE IF NOT EXISTS projects (
    id              TEXT PRIMARY KEY NOT NULL,
    organization_id TEXT NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    created_at      TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (organization_id, slug)
);

-- Kept for backward compatibility with the CP1 hierarchy. The CP5 flat
-- `environments` table below is independent of this legacy table.
CREATE TABLE IF NOT EXISTS legacy_environments (
    id         TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    slug       TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (project_id, slug)
);

CREATE TABLE IF NOT EXISTS services (
    id             TEXT PRIMARY KEY NOT NULL,
    environment_id TEXT NOT NULL REFERENCES legacy_environments (id) ON DELETE CASCADE,
    slug           TEXT NOT NULL,
    name           TEXT NOT NULL,
    created_at     TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (environment_id, slug)
);

CREATE TABLE IF NOT EXISTS provider_connections (
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
CREATE TABLE IF NOT EXISTS host_agents (
    id              TEXT PRIMARY KEY NOT NULL,
    name            TEXT NOT NULL UNIQUE,
    agent_url       TEXT NOT NULL UNIQUE,
    encrypted_token TEXT,
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    last_checked_at TEXT,
    last_error      TEXT,
    created_at      TEXT NOT NULL CHECK (length(created_at) > 0)
);

CREATE TABLE IF NOT EXISTS stack_definitions (
    id          TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL REFERENCES provider_connections (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,
    source      TEXT NOT NULL,
    yaml        TEXT NOT NULL,
    created_at  TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at  TEXT NOT NULL CHECK (length(updated_at) > 0),
    UNIQUE (provider_id, name)
);

CREATE TABLE IF NOT EXISTS audit_events (
    id          TEXT PRIMARY KEY NOT NULL,
    action      TEXT NOT NULL,
    entity_kind TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    actor       TEXT NOT NULL,
    created_at  TEXT NOT NULL CHECK (length(created_at) > 0)
);

CREATE TRIGGER IF NOT EXISTS audit_events_no_update
BEFORE UPDATE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit events are immutable');
END;

CREATE TRIGGER IF NOT EXISTS audit_events_no_delete
BEFORE DELETE ON audit_events
BEGIN
    SELECT RAISE(ABORT, 'audit events are immutable');
END;

-- CP5: environments (flat, not nested)
CREATE TABLE IF NOT EXISTS environments (
    id                 TEXT PRIMARY KEY NOT NULL,
    name               TEXT NOT NULL UNIQUE,
    host_id            TEXT NOT NULL REFERENCES host_agents (id),
    build_type         TEXT NOT NULL CHECK (build_type IN ('compose', 'dockerfile', 'nixpacks')),
    source_type        TEXT NOT NULL CHECK (source_type IN ('git', 'upload')),
    git_url            TEXT,
    git_branch         TEXT DEFAULT 'main',
    git_deploy_key     TEXT,
    upload_path        TEXT,
    registry_mode      TEXT NOT NULL DEFAULT 'local' CHECK (registry_mode IN ('local', 'push')),
    registry_url       TEXT,
    registry_credentials TEXT,
    webhook_secret     TEXT,
    webhook_enabled    INTEGER NOT NULL DEFAULT 0,
    auto_deploy        INTEGER NOT NULL DEFAULT 0,
    status             TEXT NOT NULL DEFAULT 'idle'
                       CHECK (status IN ('idle', 'building', 'deploying', 'running', 'failed')),
    created_at         TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at         TEXT NOT NULL CHECK (length(updated_at) > 0)
);

-- CP5: deployments (versioned)
CREATE TABLE IF NOT EXISTS deployments (
    id                 TEXT PRIMARY KEY NOT NULL,
    environment_id     TEXT NOT NULL REFERENCES environments (id),
    version            INTEGER NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending', 'building', 'deploying', 'running', 'failed', 'rolled_back')),
    trigger_type       TEXT NOT NULL CHECK (trigger_type IN ('manual', 'webhook')),
    git_commit_sha     TEXT,
    git_commit_message TEXT,
    build_output       TEXT,
    deploy_output      TEXT,
    started_at         TEXT NOT NULL,
    completed_at       TEXT,
    created_at         TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (environment_id, version)
);

-- CP5: deployment logs
CREATE TABLE IF NOT EXISTS deployment_logs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    deployment_id TEXT NOT NULL REFERENCES deployments (id),
    timestamp     TEXT NOT NULL,
    level         TEXT NOT NULL CHECK (level IN ('info', 'warn', 'error')),
    message       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_environments_host ON environments (host_id);
CREATE INDEX IF NOT EXISTS idx_deployments_environment ON deployments (environment_id, version);
CREATE INDEX IF NOT EXISTS idx_deployment_logs_deployment ON deployment_logs (deployment_id);
"#;

/// Handle to the catalog database.
pub struct Catalog {
    connection: Connection,
    key: [u8; KEY_LEN],
}

impl Catalog {
    /// Opens (or creates) the catalog at `path` and applies pending migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let connection =
            Connection::open(path).with_context(|| format!("open catalog {}", path.display()))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .context("enable foreign keys")?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .context("configure busy timeout")?;

        let mut catalog = Self {
            connection,
            key: load_or_create_key(path)?,
        };
        catalog.migrate()?;
        Ok(catalog)
    }

    /// Current schema version stored in the database.
    pub fn schema_version(&self) -> Result<i64> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .context("read schema version")
    }

    fn migrate(&mut self) -> Result<()> {
        let current: i64 = self.schema_version()?;
        if current > SCHEMA_VERSION {
            bail!(
                "catalog schema version {current} is newer than supported version {SCHEMA_VERSION}"
            );
        }
        if current == SCHEMA_VERSION {
            return Ok(());
        }

        let transaction = self.connection.transaction()?;
        rename_legacy_environments(&transaction)?;
        transaction
            .execute_batch(SCHEMA)
            .context("create catalog schema")?;
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .context("set schema version")?;
        transaction.commit()?;
        Ok(())
    }

    fn encrypt_secret(&self, secret: &str) -> Result<String> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), secret.as_bytes())
            .map_err(|_| anyhow!("encrypt provider secret"))?;
        let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&ciphertext);
        Ok(BASE64.encode(payload))
    }

    fn decrypt_secret(&self, encoded: &str) -> Result<String> {
        let payload = BASE64
            .decode(encoded)
            .context("decode provider secret encoding")?;
        if payload.len() < NONCE_LEN {
            bail!("provider secret ciphertext is truncated");
        }
        let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| anyhow!("decrypt provider secret"))?;
        String::from_utf8(plaintext).context("provider secret is not valid UTF-8")
    }
}

/// Moves the CP1 hierarchy `environments` table aside so the CP5 flat
/// `environments` table can claim its name.
///
/// SQLite rewrites the `services` foreign key to point at
/// `legacy_environments`, so legacy rows stay fully consistent. A no-op when
/// the rename already happened.
fn rename_legacy_environments(transaction: &Transaction<'_>) -> Result<()> {
    let renamed_already: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master \
         WHERE type = 'table' AND name = 'legacy_environments')",
        [],
        |row| row.get(0),
    )?;
    if renamed_already {
        return Ok(());
    }
    let legacy_present: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master \
         WHERE type = 'table' AND name = 'environments')",
        [],
        |row| row.get(0),
    )?;
    if legacy_present {
        transaction.execute("ALTER TABLE environments RENAME TO legacy_environments", [])?;
    }
    Ok(())
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn key_path(database: &Path) -> PathBuf {
    database.with_extension("key")
}

fn load_or_create_key(database: &Path) -> Result<[u8; KEY_LEN]> {
    let path = key_path(database);
    match fs::read(&path) {
        Ok(bytes) => key_from_bytes(&path, &bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut key = [0u8; KEY_LEN];
            rand::rngs::OsRng.fill_bytes(&mut key);
            match create_key_file(&path, &key) {
                Ok(()) => Ok(key),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let bytes = fs::read(&path)
                        .with_context(|| format!("read catalog key {}", path.display()))?;
                    key_from_bytes(&path, &bytes)
                }
                Err(error) => {
                    Err(error).with_context(|| format!("create catalog key {}", path.display()))
                }
            }
        }
        Err(error) => Err(error).with_context(|| format!("read catalog key {}", path.display())),
    }
}

fn key_from_bytes(path: &Path, bytes: &[u8]) -> Result<[u8; KEY_LEN]> {
    if bytes.len() != KEY_LEN {
        bail!(
            "catalog key {} must be {KEY_LEN} bytes, found {}",
            path.display(),
            bytes.len()
        );
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(bytes);
    Ok(key)
}

fn create_key_file(path: &Path, key: &[u8; KEY_LEN]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(key)?;
    file.sync_all()?;
    Ok(())
}

fn record_audit(
    transaction: &Transaction<'_>,
    action: &str,
    entity_kind: &str,
    entity_id: &str,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO audit_events (id, action, entity_kind, entity_id, actor, created_at) \
         VALUES (?1, ?2, ?3, ?4, 'system', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        params![new_id(), action, entity_kind, entity_id],
    )?;
    Ok(())
}

fn record_created(transaction: &Transaction<'_>, entity_kind: &str, entity_id: &str) -> Result<()> {
    record_audit(transaction, "create", entity_kind, entity_id)
}
