//! App and deployment persistence for the CP9 flat model.
//!
//! Apps bind a build recipe to an enrolled host agent. Deployments
//! are immutable, versioned records; logs attach to a single deployment.

use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};

use super::deployments::validate_build_type;
use super::Catalog;

/// A CP9 app: build recipe bound to an enrolled host agent.
#[derive(Debug, Clone)]
pub struct App {
    pub id: String,
    pub name: String,
    pub host_id: String,
    pub build_type: String,
    pub github_repo: Option<String>,
    pub github_branch: Option<String>,
    pub github_path: Option<String>,
    pub github_dockerfile: Option<String>,
    pub github_auto_deploy: bool,
    pub container_port: Option<u16>,
    /// Host port the current deployment publishes (None = not running).
    pub host_port: Option<u16>,
    /// JSON object of environment variables passed to the container.
    pub env_json: String,
    pub status: String,
    pub webhook_secret: Option<String>,
    pub webhook_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Validates that `value` is one of the accepted app statuses.
pub fn validate_app_status(value: &str) -> Result<()> {
    if matches!(
        value,
        "idle" | "building" | "deploying" | "running" | "failed"
    ) {
        return Ok(());
    }
    bail!("app status must be one of idle, building, deploying, running, failed");
}

/// Validates that `value` is one of the accepted deployment statuses.
/// Container port must be a valid TCP port if provided (1-65535).
fn validate_port(port: Option<u16>) -> Result<()> {
    if port == Some(0) {
        bail!("container port must be between 1 and 65535");
    }
    Ok(())
}

pub fn validate_deployment_status(value: &str) -> Result<()> {
    if matches!(
        value,
        "pending" | "building" | "deploying" | "running" | "failed" | "cancelled" | "rolled_back"
    ) {
        return Ok(());
    }
    bail!(
        "deployment status must be one of pending, building, deploying, running, failed, \
         rolled_back"
    );
}

/// Validates that `value` is one of the accepted log levels.
pub fn validate_log_level(value: &str) -> Result<()> {
    if matches!(value, "info" | "warn" | "error") {
        return Ok(());
    }
    bail!("log level must be info, warn, or error");
}

/// Validates an app name: non-empty ASCII slug, 1–64 characters.
pub fn validate_app_name(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 64 {
        bail!("app name must be 1-64 characters");
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("app name must contain only ASCII letters, digits, '-', '_', or '.'");
    }
    Ok(())
}

fn read_app_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<App> {
    Ok(App {
        id: row.get(0)?,
        name: row.get(1)?,
        host_id: row.get(2)?,
        build_type: row.get(3)?,
        github_repo: row.get(4)?,
        github_branch: row.get(5)?,
        github_path: row.get(6)?,
        github_dockerfile: row.get(7)?,
        github_auto_deploy: row.get::<_, i64>(8)? != 0,
        container_port: row.get(9)?,
        host_port: row.get::<_, Option<i64>>(10)?.map(|v| v as u16),
        env_json: row.get(11)?,
        status: row.get(12)?,
        webhook_secret: row.get(13)?,
        webhook_enabled: row.get::<_, i64>(14)? != 0,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

const APP_COLUMNS: &str = "id, name, host_id, build_type, github_repo, \
     github_branch, github_path, github_dockerfile, github_auto_deploy, container_port, host_port, \
     env_json, status, webhook_secret, webhook_enabled, created_at, updated_at";

impl Catalog {
    /// Replaces the app's container environment variables (JSON object).
    pub fn set_app_env_json(&self, id: &str, env_json: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE apps SET env_json = ?2 WHERE id = ?1",
            params![id, env_json],
        )?;
        Ok(())
    }

    /// Persists the host port published by the latest deployment.
    pub fn set_app_host_port(&self, id: &str, host_port: Option<u16>) -> Result<()> {
        self.connection.execute(
            "UPDATE apps SET host_port = ?2 WHERE id = ?1",
            params![id, host_port.map(i64::from)],
        )?;
        Ok(())
    }

    /// Creates a CP9 app bound to an enrolled host agent.
    ///
    /// The host must exist and be enabled. The name must be a unique ASCII
    /// slug.
    #[allow(clippy::too_many_arguments)]
    pub fn create_app(
        &mut self,
        name: &str,
        host_id: &str,
        build_type: &str,
        github_repo: Option<&str>,
        github_branch: Option<&str>,
        github_path: Option<&str>,
        github_dockerfile: Option<&str>,
        github_auto_deploy: bool,
        container_port: Option<u16>,
    ) -> Result<String> {
        let name = name.trim();
        validate_app_name(name)?;
        validate_build_type(build_type)?;
        validate_port(container_port)?;

        let id = super::new_id();
        let transaction = self.connection.transaction()?;
        ensure_enabled_host(&transaction, host_id)?;
        if app_name_taken(&transaction, name, None)? {
            bail!("app {name} already exists");
        }
        transaction.execute(
            "INSERT INTO apps \
                 (id, name, host_id, build_type, github_repo, github_branch, \
                  github_path, github_dockerfile, github_auto_deploy, container_port, \
                  status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'idle', \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![
                id,
                name,
                host_id,
                build_type,
                github_repo.map(str::trim).filter(|value| !value.is_empty()),
                github_branch
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                github_path.map(str::trim).filter(|value| !value.is_empty()),
                github_dockerfile
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                i64::from(github_auto_deploy),
                container_port,
            ],
        )?;
        super::record_created(&transaction, "app", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists CP9 apps ordered by creation.
    pub fn list_apps(&self) -> Result<Vec<App>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {APP_COLUMNS} FROM apps ORDER BY created_at, rowid"
        ))?;
        let rows = statement
            .query_map([], read_app_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Loads a single CP5 app by id.
    pub fn get_app(&self, id: &str) -> Result<Option<App>> {
        self.connection
            .query_row(
                &format!("SELECT {APP_COLUMNS} FROM apps WHERE id = ?1"),
                params![id],
                read_app_row,
            )
            .optional()
            .context("load app")
    }

    /// Replaces the mutable fields of a CP9 app.
    #[allow(clippy::too_many_arguments)]
    pub fn update_app(
        &mut self,
        id: &str,
        name: &str,
        host_id: &str,
        build_type: &str,
        github_repo: Option<&str>,
        github_branch: Option<&str>,
        github_path: Option<&str>,
        github_dockerfile: Option<&str>,
        github_auto_deploy: bool,
        container_port: Option<u16>,
        env_json: &str,
    ) -> Result<()> {
        let name = name.trim();
        validate_app_name(name)?;
        let parsed: serde_json::Value = serde_json::from_str(env_json)
            .map_err(|error| anyhow::anyhow!("env must be a JSON object: {error}"))?;
        if !parsed.is_object() {
            bail!("env must be a JSON object");
        }
        validate_build_type(build_type)?;
        validate_port(container_port)?;

        let transaction = self.connection.transaction()?;
        ensure_enabled_host(&transaction, host_id)?;
        if app_name_taken(&transaction, name, Some(id))? {
            bail!("app {name} already exists");
        }
        let updated = transaction.execute(
            "UPDATE apps \
             SET name = ?2, host_id = ?3, build_type = ?4, github_repo = ?5, \
                 github_branch = ?6, github_path = ?7, github_dockerfile = ?8, \
                 github_auto_deploy = ?9, container_port = ?10, env_json = ?11, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![
                id,
                name,
                host_id,
                build_type,
                github_repo.map(str::trim).filter(|value| !value.is_empty()),
                github_branch
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                github_path.map(str::trim).filter(|value| !value.is_empty()),
                github_dockerfile
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                i64::from(github_auto_deploy),
                container_port,
            ],
        )?;
        if updated == 0 {
            bail!("app {id} not found");
        }
        super::record_audit(&transaction, "update", "app", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Sets the app status to one of the allowed values.
    pub fn set_app_status(&mut self, id: &str, status: &str) -> Result<()> {
        validate_app_status(status)?;
        let updated = self.connection.execute(
            "UPDATE apps \
             SET status = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id, status],
        )?;
        if updated == 0 {
            bail!("app {id} not found");
        }
        Ok(())
    }

    /// Permanently deletes an app, its deployments, and their logs.
    pub fn delete_app(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM deployment_logs WHERE deployment_id IN \
                 (SELECT id FROM deployments WHERE app_id = ?1)",
            params![id],
        )?;
        transaction.execute("DELETE FROM deployments WHERE app_id = ?1", params![id])?;
        let deleted = transaction.execute("DELETE FROM apps WHERE id = ?1", params![id])?;
        if deleted == 0 {
            bail!("app {id} not found");
        }
        super::record_audit(&transaction, "delete", "app", id)?;
        transaction.commit()?;
        Ok(())
    }
}

fn ensure_enabled_host(transaction: &rusqlite::Transaction<'_>, host_id: &str) -> Result<()> {
    let enabled: Option<i64> = transaction
        .query_row(
            "SELECT enabled FROM host_agents WHERE id = ?1",
            params![host_id],
            |row| row.get(0),
        )
        .optional()?;
    match enabled {
        Some(1) => Ok(()),
        Some(_) => bail!("host agent {host_id} is disabled"),
        None => bail!("host agent {host_id} not found"),
    }
}

fn app_name_taken(
    transaction: &rusqlite::Transaction<'_>,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<bool> {
    let taken: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM apps WHERE name = ?1 AND (?2 IS NULL OR id <> ?2))",
        params![name, exclude_id],
        |row| row.get(0),
    )?;
    Ok(taken)
}
