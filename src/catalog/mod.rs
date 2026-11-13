//! Persistent catalog for apps, hosts, providers, and deployments.
//!
//! The catalog is a SQLite database. Its schema is versioned with
//! `PRAGMA user_version`; opening an up-to-date database is a no-op.

mod apps;
mod deployments;
mod github;
mod hosts;
mod providers;

/// Upper bound for [`Catalog::list_recent_deployments`].
pub const DEPLOYMENT_PAGE_LIMIT: u32 = 100;

pub use apps::{
    validate_app_name, validate_app_status, validate_deployment_status, validate_log_level, App,
};
pub use deployments::{
    validate_build_type, validate_registry_mode, validate_source_type, Deployment, DeploymentLog,
};
pub use github::{GithubApp, GithubConnection};
pub use hosts::HostAgent;
pub use providers::ProviderConnection;

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
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use uuid::Uuid;

/// Schema version written to `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 12;

/// Capabilities a provider connection may be granted.
const CAPABILITIES: [&str; 3] = ["read", "operate", "admin"];

/// Length of a ChaCha20-Poly1305 key and nonce in bytes.
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

const SCHEMA: &str = r#"
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

-- v9: apps (Railway-style; replaces environments)
CREATE TABLE IF NOT EXISTS apps (
    id                 TEXT PRIMARY KEY NOT NULL,
    name               TEXT NOT NULL UNIQUE,
    host_id            TEXT REFERENCES host_agents (id),
    build_type         TEXT NOT NULL DEFAULT 'dockerfile' CHECK (build_type IN ('dockerfile', 'nixpacks')),
    -- GitHub source (CP9)
    github_repo        TEXT,
    github_branch      TEXT DEFAULT 'main',
    github_path        TEXT,
    github_dockerfile  TEXT DEFAULT 'Dockerfile',
    github_auto_deploy INTEGER NOT NULL DEFAULT 0,
    -- runtime
    container_port     INTEGER,
    host_port          INTEGER,
    env_json           TEXT NOT NULL DEFAULT '{}',
    webhook_secret     TEXT,
    webhook_enabled    INTEGER NOT NULL DEFAULT 0,
    status             TEXT NOT NULL DEFAULT 'idle'
                       CHECK (status IN ('idle', 'building', 'deploying', 'running', 'failed')),
    created_at         TEXT NOT NULL CHECK (length(created_at) > 0),
    updated_at         TEXT NOT NULL CHECK (length(updated_at) > 0)
);

-- v9: deployments (versioned, per app)
CREATE TABLE IF NOT EXISTS deployments (
    id                 TEXT PRIMARY KEY NOT NULL,
    app_id             TEXT NOT NULL REFERENCES apps (id),
    version            INTEGER NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending', 'building', 'deploying', 'running', 'failed', 'cancelled', 'rolled_back')),
    trigger_type       TEXT NOT NULL CHECK (trigger_type IN ('manual', 'webhook')),
    git_commit_sha     TEXT,
    git_commit_message TEXT,
    build_output       TEXT,
    deploy_output      TEXT,
    started_at         TEXT NOT NULL,
    completed_at       TEXT,
    created_at         TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (app_id, version)
);

-- CP5: deployment logs
CREATE TABLE IF NOT EXISTS deployment_logs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    deployment_id TEXT NOT NULL REFERENCES deployments (id),
    timestamp     TEXT NOT NULL,
    level         TEXT NOT NULL CHECK (level IN ('info', 'warn', 'error')),
    message       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_deployment_logs_deployment ON deployment_logs (deployment_id);
"#;

/// v9 indexes — created only after apps/deployments exist in their final shape.
const APP_INDEXES: &str = "CREATE INDEX IF NOT EXISTS idx_apps_host ON apps (host_id);
CREATE INDEX IF NOT EXISTS idx_deployments_app ON deployments (app_id, version);";

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
        transaction
            .execute_batch(SCHEMA)
            .context("create catalog schema")?;
        if current == 0 {
            transaction
                .execute_batch(APP_INDEXES)
                .context("create app indexes")?;
        }
        if current < 6 {
            migrate_v6_github(&transaction)?;
        }
        if (1..7).contains(&current) {
            migrate_v7_github_installation(&transaction)?;
        }
        if (1..8).contains(&current) {
            transaction
                .execute("ALTER TABLE github_app ADD COLUMN app_slug TEXT", [])
                .context("add github_app.app_slug")?;
        }
        if (1..9).contains(&current) {
            migrate_v9_apps(&transaction)?;
        }
        if (9..10).contains(&current) {
            transaction
                .execute("ALTER TABLE apps ADD COLUMN host_port INTEGER", [])
                .context("add apps.host_port")?;
        }
        // settings_kv arrived with v6 on some databases; guarantee it exists
        // for every database crossing the v10 boundary.
        if (1..12).contains(&current) {
            transaction
                .execute(
                    "ALTER TABLE apps ADD COLUMN env_json TEXT NOT NULL DEFAULT '{}'",
                    [],
                )
                .context("add apps.env_json")?;
        }
        if (1..11).contains(&current) {
            transaction
                .execute(
                    "CREATE TABLE IF NOT EXISTS settings_kv (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL)",
                    [],
                )
                .context("create settings_kv")?;
        }
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .context("set schema version")?;
        transaction.commit()?;
        Ok(())
    }

    /// Reads one settings key.
    pub fn settings_get(&self, key: &str) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT value FROM settings_kv WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .context("read settings key")
    }

    /// Writes one settings key.
    /// Writes a settings key with the value encrypted via the vault.
    pub fn settings_set_encrypted(&self, key: &str, value: &str) -> Result<()> {
        let encrypted = self.encrypt_secret(value)?;
        self.settings_set(key, &encrypted)
    }

    pub fn settings_set(&self, key: &str, value: &str) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO settings_kv (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .context("write settings key")?;
        Ok(())
    }

    pub fn encrypt_secret(&self, secret: &str) -> Result<String> {
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

    pub fn decrypt_secret(&self, encoded: &str) -> Result<String> {
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

/// v9: replace environments with apps; wipe all deploy data. Fresh start.
fn migrate_v9_apps(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "DROP TABLE IF EXISTS deployment_logs;
         DROP TABLE IF EXISTS deployments;
         DROP TABLE IF EXISTS legacy_environments;
         DROP TABLE IF EXISTS environments;
         DROP TABLE IF EXISTS services;
         DROP TABLE IF EXISTS stack_definitions;
         CREATE TABLE IF NOT EXISTS apps (
            id                 TEXT PRIMARY KEY NOT NULL,
            name               TEXT NOT NULL UNIQUE,
            host_id            TEXT REFERENCES host_agents (id),
            build_type         TEXT NOT NULL DEFAULT 'dockerfile' CHECK (build_type IN ('dockerfile', 'nixpacks')),
            github_repo        TEXT,
            github_branch      TEXT DEFAULT 'main',
            github_path        TEXT,
            github_dockerfile  TEXT DEFAULT 'Dockerfile',
            github_auto_deploy INTEGER NOT NULL DEFAULT 0,
            container_port     INTEGER,
            host_port          INTEGER,
            webhook_secret     TEXT,
            webhook_enabled    INTEGER NOT NULL DEFAULT 0,
            status             TEXT NOT NULL DEFAULT 'idle'
                               CHECK (status IN ('idle', 'building', 'deploying', 'running', 'failed')),
            created_at         TEXT NOT NULL CHECK (length(created_at) > 0),
            updated_at         TEXT NOT NULL CHECK (length(updated_at) > 0)
         );
         CREATE TABLE IF NOT EXISTS deployments (
            id                 TEXT PRIMARY KEY NOT NULL,
            app_id             TEXT NOT NULL REFERENCES apps (id),
            version            INTEGER NOT NULL,
            status             TEXT NOT NULL DEFAULT 'pending'
                               CHECK (status IN ('pending', 'building', 'deploying', 'running', 'failed', 'cancelled', 'rolled_back')),
            trigger_type       TEXT NOT NULL CHECK (trigger_type IN ('manual', 'webhook')),
            git_commit_sha     TEXT,
            git_commit_message TEXT,
            build_output       TEXT,
            deploy_output      TEXT,
            started_at         TEXT NOT NULL,
            completed_at       TEXT,
            created_at         TEXT NOT NULL CHECK (length(created_at) > 0),
            UNIQUE (app_id, version)
         );
         CREATE TABLE IF NOT EXISTS deployment_logs (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            deployment_id TEXT NOT NULL REFERENCES deployments (id),
            timestamp     TEXT NOT NULL,
            level         TEXT NOT NULL CHECK (level IN ('info', 'warn', 'error')),
            message       TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_apps_host ON apps (host_id);
         CREATE INDEX IF NOT EXISTS idx_deployments_app ON deployments (app_id, version);
         CREATE INDEX IF NOT EXISTS idx_deployment_logs_deployment ON deployment_logs (deployment_id);",
    )?;
    Ok(())
}

/// v7: installation becomes optional — the manifest flow creates the app
/// before the user installs it, so installation_id/installed_at are nullable.
fn migrate_v7_github_installation(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_app_v7 (
            id              TEXT PRIMARY KEY NOT NULL,
            app_id          INTEGER NOT NULL,
            client_id       TEXT NOT NULL,
            client_secret   TEXT NOT NULL,
            webhook_secret  TEXT NOT NULL,
            private_key_pem TEXT NOT NULL,
            installation_id INTEGER,
            installed_at    TEXT
        );
        INSERT OR IGNORE INTO github_app_v7
            SELECT id, app_id, client_id, client_secret, webhook_secret,
                   private_key_pem, installation_id, installed_at FROM github_app;
        DROP TABLE github_app;
        ALTER TABLE github_app_v7 RENAME TO github_app;",
    )?;
    Ok(())
}

/// CP9 schema v6: GitHub integration tables.
fn migrate_v6_github(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_app (
            id              TEXT PRIMARY KEY NOT NULL,
            app_id          INTEGER NOT NULL,
            client_id       TEXT NOT NULL,
            client_secret   TEXT NOT NULL,
            webhook_secret  TEXT NOT NULL,
            private_key_pem TEXT NOT NULL,
            installation_id INTEGER,
            installed_at    TEXT
        );
        CREATE TABLE IF NOT EXISTS github_connections (
            id              TEXT PRIMARY KEY NOT NULL,
            user_id         TEXT NOT NULL,
            github_login    TEXT NOT NULL,
            github_user_id  INTEGER NOT NULL,
            access_token    TEXT NOT NULL,
            refresh_token   TEXT,
            token_expires_at TEXT NOT NULL,
            created_at      TEXT NOT NULL CHECK (length(created_at) > 0)
        );
        CREATE TABLE IF NOT EXISTS webhook_deliveries (
            delivery_id TEXT PRIMARY KEY NOT NULL,
            received_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings_kv (
            key   TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        );",
    )?;
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
