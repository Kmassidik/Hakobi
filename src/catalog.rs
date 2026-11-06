//! Persistent catalog for the organization/project/environment/service tree.
//!
//! The catalog is a SQLite database. Its schema is versioned with
//! `PRAGMA user_version`; opening an up-to-date database is a no-op.

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

use crate::stacks::{validate_definition, StackDefinition, StackKind};

/// Schema version written to `PRAGMA user_version`.
const SCHEMA_VERSION: i64 = 3;

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

CREATE TABLE IF NOT EXISTS environments (
    id         TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    slug       TEXT NOT NULL,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) > 0),
    UNIQUE (project_id, slug)
);

CREATE TABLE IF NOT EXISTS services (
    id             TEXT PRIMARY KEY NOT NULL,
    environment_id TEXT NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
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
"#;

/// A service registered in the catalog.
#[derive(Debug, Clone)]
pub struct Service {
    pub id: String,
    pub environment_id: String,
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

/// A provider connection registered in the catalog.
///
/// The encrypted credential is intentionally absent from this view.
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

/// An immutable record of a catalog mutation.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub id: String,
    pub action: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub actor: String,
    pub created_at: String,
}

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

    /// Creates an organization and records an audit event.
    pub fn create_organization(&mut self, slug: &str, name: &str) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO organizations (id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, slug, name],
        )?;
        record_created(&transaction, "organization", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Creates a project inside an organization and records an audit event.
    pub fn create_project(
        &mut self,
        organization_id: &str,
        slug: &str,
        name: &str,
    ) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO projects (id, organization_id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, organization_id, slug, name],
        )?;
        record_created(&transaction, "project", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Creates an environment inside a project and records an audit event.
    pub fn create_environment(
        &mut self,
        project_id: &str,
        slug: &str,
        name: &str,
    ) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO environments (id, project_id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, project_id, slug, name],
        )?;
        record_created(&transaction, "environment", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Creates a service inside an environment and records an audit event.
    pub fn create_service(
        &mut self,
        environment_id: &str,
        slug: &str,
        name: &str,
    ) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO services (id, environment_id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, environment_id, slug, name],
        )?;
        record_created(&transaction, "service", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists services ordered by creation.
    pub fn list_services(&self) -> Result<Vec<Service>> {
        let mut statement = self.connection.prepare(
            "SELECT id, environment_id, slug, name, created_at \
             FROM services ORDER BY created_at, rowid",
        )?;
        let services = statement
            .query_map([], |row| {
                Ok(Service {
                    id: row.get(0)?,
                    environment_id: row.get(1)?,
                    slug: row.get(2)?,
                    name: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(services)
    }

    /// Lists audit events ordered by creation.
    pub fn list_audit_events(&self) -> Result<Vec<AuditEvent>> {
        let mut statement = self.connection.prepare(
            "SELECT id, action, entity_kind, entity_id, actor, created_at \
             FROM audit_events ORDER BY created_at, rowid",
        )?;
        let events = statement
            .query_map([], |row| {
                Ok(AuditEvent {
                    id: row.get(0)?,
                    action: row.get(1)?,
                    entity_kind: row.get(2)?,
                    entity_id: row.get(3)?,
                    actor: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    /// Creates an enabled provider connection with an encrypted credential.
    pub fn create_provider_connection(
        &mut self,
        name: &str,
        kind: &str,
        endpoint: &str,
        secret: &str,
    ) -> Result<String> {
        let id = new_id();
        let encrypted_secret = self.encrypt_secret(secret)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO provider_connections \
                 (id, name, kind, endpoint, encrypted_secret, capabilities, enabled, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, '[]', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, name, kind, endpoint, encrypted_secret],
        )?;
        record_created(&transaction, "provider_connection", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists provider connections without revealing credentials.
    pub fn list_provider_connections(&self) -> Result<Vec<ProviderConnection>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, kind, endpoint, capabilities, enabled, \
                    last_checked_at, last_error, created_at \
             FROM provider_connections ORDER BY created_at, rowid",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)? != 0,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(
                |(
                    id,
                    name,
                    kind,
                    endpoint,
                    capabilities,
                    enabled,
                    last_checked_at,
                    last_error,
                    created_at,
                )| {
                    Ok(ProviderConnection {
                        id,
                        name,
                        kind,
                        endpoint,
                        capabilities: serde_json::from_str(&capabilities)
                            .context("decode provider capabilities")?,
                        enabled,
                        last_checked_at,
                        last_error,
                        created_at,
                    })
                },
            )
            .collect()
    }

    /// Decrypts the stored credential for a provider connection.
    pub fn provider_secret(&self, id: &str) -> Result<String> {
        let encrypted_secret = self
            .connection
            .query_row(
                "SELECT encrypted_secret FROM provider_connections WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .with_context(|| format!("provider connection {id} not found"))?
            .with_context(|| format!("provider connection {id} has no stored secret"))?;
        self.decrypt_secret(&encrypted_secret)
    }

    /// Replaces the credential for a provider connection and audits the change.
    pub fn rotate_provider_secret(&mut self, id: &str, secret: &str) -> Result<()> {
        let encrypted_secret = self.encrypt_secret(secret)?;
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE provider_connections SET encrypted_secret = ?2 WHERE id = ?1",
            params![id, encrypted_secret],
        )?;
        if updated == 0 {
            bail!("provider connection {id} not found");
        }
        record_audit(&transaction, "rotate", "provider_connection", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Disables a provider connection and destroys its stored credential.
    pub fn revoke_provider_connection(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE provider_connections SET enabled = 0, encrypted_secret = NULL WHERE id = ?1",
            params![id],
        )?;
        if updated == 0 {
            bail!("provider connection {id} not found");
        }
        record_audit(&transaction, "revoke", "provider_connection", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Enables or disables a provider without discarding its credential.
    pub fn set_provider_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE provider_connections SET enabled = ?2 WHERE id = ?1",
            params![id, i64::from(enabled)],
        )?;
        if updated == 0 {
            bail!("provider connection {id} not found");
        }
        record_audit(
            &transaction,
            if enabled { "enable" } else { "disable" },
            "provider_connection",
            id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Permanently deletes a provider connection and audits the removal.
    pub fn delete_provider_connection(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let deleted = transaction.execute(
            "DELETE FROM provider_connections WHERE id = ?1",
            params![id],
        )?;
        if deleted == 0 {
            bail!("provider connection {id} not found");
        }
        record_audit(&transaction, "delete", "provider_connection", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Replaces the capability scope of a provider connection.
    pub fn set_provider_capabilities(&mut self, id: &str, capabilities: &[&str]) -> Result<()> {
        for capability in capabilities {
            if !CAPABILITIES.contains(capability) {
                bail!("unsupported provider capability: {capability}");
            }
        }
        let capabilities = serde_json::to_string(capabilities).context("encode capabilities")?;
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE provider_connections SET capabilities = ?2 WHERE id = ?1",
            params![id, capabilities],
        )?;
        if updated == 0 {
            bail!("provider connection {id} not found");
        }
        record_audit(
            &transaction,
            "update_capabilities",
            "provider_connection",
            id,
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Records the latest health check for a provider connection.
    pub fn record_provider_health(&mut self, id: &str, error: Option<&str>) -> Result<()> {
        let updated = self.connection.execute(
            "UPDATE provider_connections \
             SET last_checked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), last_error = ?2 \
             WHERE id = ?1",
            params![id, error],
        )?;
        if updated == 0 {
            bail!("provider connection {id} not found");
        }
        Ok(())
    }

    /// Creates a stack definition for a provider and records an audit event.
    pub fn create_stack(
        &mut self,
        provider_id: &str,
        name: &str,
        kind: StackKind,
        source: &str,
        yaml: &str,
    ) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            bail!("stack name must not be empty");
        }
        validate_definition(&kind, yaml)?;

        let id = new_id();
        let transaction = self.connection.transaction()?;
        ensure_provider_exists(&transaction, provider_id)?;
        if stack_name_taken(&transaction, provider_id, name, None)? {
            bail!("stack {name} already exists for provider {provider_id}");
        }
        transaction.execute(
            "INSERT INTO stack_definitions \
                 (id, provider_id, name, kind, source, yaml, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, provider_id, name, kind.as_str(), source, yaml],
        )?;
        record_created(&transaction, "stack_definition", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists stack definitions ordered by creation.
    pub fn list_stacks(&self) -> Result<Vec<StackDefinition>> {
        let mut statement = self.connection.prepare(
            "SELECT id, provider_id, name, kind, source, yaml, created_at, updated_at \
             FROM stack_definitions ORDER BY created_at, rowid",
        )?;
        let rows = statement
            .query_map([], read_stack_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(StackRow::into_definition).collect()
    }

    /// Loads a stack definition by id.
    pub fn get_stack(&self, id: &str) -> Result<Option<StackDefinition>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, provider_id, name, kind, source, yaml, created_at, updated_at \
                 FROM stack_definitions WHERE id = ?1",
                params![id],
                read_stack_row,
            )
            .optional()?;
        match row {
            Some(row) => Ok(Some(row.into_definition()?)),
            None => Ok(None),
        }
    }

    /// Replaces a stack definition and records an audit event.
    pub fn update_stack(
        &mut self,
        id: &str,
        name: &str,
        kind: StackKind,
        source: &str,
        yaml: &str,
    ) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("stack name must not be empty");
        }
        validate_definition(&kind, yaml)?;

        let transaction = self.connection.transaction()?;
        let provider_id = transaction
            .query_row(
                "SELECT provider_id FROM stack_definitions WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .with_context(|| format!("stack {id} not found"))?;
        if stack_name_taken(&transaction, &provider_id, name, Some(id))? {
            bail!("stack {name} already exists for provider {provider_id}");
        }
        transaction.execute(
            "UPDATE stack_definitions \
             SET name = ?2, kind = ?3, source = ?4, yaml = ?5, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id, name, kind.as_str(), source, yaml],
        )?;
        record_audit(&transaction, "update", "stack_definition", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Permanently deletes a stack definition and records an audit event.
    pub fn delete_stack(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let deleted =
            transaction.execute("DELETE FROM stack_definitions WHERE id = ?1", params![id])?;
        if deleted == 0 {
            bail!("stack {id} not found");
        }
        record_audit(&transaction, "delete", "stack_definition", id)?;
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
        transaction
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .context("set schema version")?;
        transaction.commit()?;
        Ok(())
    }
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

struct StackRow {
    id: String,
    provider_id: String,
    name: String,
    kind: String,
    source: String,
    yaml: String,
    created_at: String,
    updated_at: String,
}

impl StackRow {
    fn into_definition(self) -> Result<StackDefinition> {
        let kind = self
            .kind
            .parse::<StackKind>()
            .map_err(|error: anyhow::Error| {
                error.context(format!("decode stack kind for stack {}", self.id))
            })?;
        Ok(StackDefinition {
            id: self.id,
            provider_id: self.provider_id,
            name: self.name,
            kind,
            source: self.source,
            yaml: self.yaml,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn read_stack_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StackRow> {
    Ok(StackRow {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        source: row.get(4)?,
        yaml: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn ensure_provider_exists(transaction: &Transaction<'_>, provider_id: &str) -> Result<()> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_connections WHERE id = ?1)",
        params![provider_id],
        |row| row.get(0),
    )?;
    if !exists {
        bail!("provider connection {provider_id} not found");
    }
    Ok(())
}

fn stack_name_taken(
    transaction: &Transaction<'_>,
    provider_id: &str,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<bool> {
    let taken: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM stack_definitions \
         WHERE provider_id = ?1 AND name = ?2 AND (?3 IS NULL OR id <> ?3))",
        params![provider_id, name, exclude_id],
        |row| row.get(0),
    )?;
    Ok(taken)
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
