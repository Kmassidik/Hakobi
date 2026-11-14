use anyhow::{bail, Context, Result};
use rusqlite::{Connection, Transaction};

/// Schema version written to `PRAGMA user_version`.
pub(super) const SCHEMA_VERSION: i64 = 12;

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

pub(super) fn migrate(connection: &mut Connection) -> Result<()> {
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .context("read schema version")?;
    if current > SCHEMA_VERSION {
        bail!("catalog schema version {current} is newer than supported version {SCHEMA_VERSION}");
    }
    if current == SCHEMA_VERSION {
        return Ok(());
    }

    let transaction = connection.transaction()?;
    apply_migrations(&transaction, current)?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .context("set schema version")?;
    transaction.commit()?;
    Ok(())
}

fn apply_migrations(transaction: &Transaction<'_>, current: i64) -> Result<()> {
    transaction
        .execute_batch(SCHEMA)
        .context("create catalog schema")?;
    if current == 0 {
        transaction
            .execute_batch(APP_INDEXES)
            .context("create app indexes")?;
    }
    migrate_github(transaction, current)?;
    migrate_apps(transaction, current)?;
    migrate_settings(transaction, current)?;
    Ok(())
}

fn migrate_github(transaction: &Transaction<'_>, current: i64) -> Result<()> {
    if current < 6 {
        migrate_v6_github(transaction)?;
    }
    if (1..7).contains(&current) {
        migrate_v7_github_installation(transaction)?;
    }
    if (1..8).contains(&current) {
        transaction
            .execute("ALTER TABLE github_app ADD COLUMN app_slug TEXT", [])
            .context("add github_app.app_slug")?;
    }
    Ok(())
}

fn migrate_apps(transaction: &Transaction<'_>, current: i64) -> Result<()> {
    if (1..9).contains(&current) {
        migrate_v9_apps(transaction)?;
    }
    if (9..10).contains(&current) {
        transaction
            .execute("ALTER TABLE apps ADD COLUMN host_port INTEGER", [])
            .context("add apps.host_port")?;
    }
    if (1..12).contains(&current) {
        transaction
            .execute(
                "ALTER TABLE apps ADD COLUMN env_json TEXT NOT NULL DEFAULT '{}'",
                [],
            )
            .context("add apps.env_json")?;
    }
    Ok(())
}

fn migrate_settings(transaction: &Transaction<'_>, current: i64) -> Result<()> {
    // settings_kv arrived with v6 on some databases; guarantee it exists
    // for every database crossing the v10 boundary.
    if (1..11).contains(&current) {
        transaction
            .execute(
                "CREATE TABLE IF NOT EXISTS settings_kv (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL)",
                [],
            )
            .context("create settings_kv")?;
    }
    Ok(())
}

/// v9: replace environments with apps; wipe all deploy data. Fresh start.
fn migrate_v9_apps(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(MIGRATE_V9_APPS)?;
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

const MIGRATE_V9_APPS: &str = "DROP TABLE IF EXISTS deployment_logs;
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
         CREATE INDEX IF NOT EXISTS idx_deployment_logs_deployment ON deployment_logs (deployment_id);";
