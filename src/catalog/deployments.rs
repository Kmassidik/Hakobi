//! Deployment persistence: versioned deployment records and their logs.

use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};

use super::apps::{validate_deployment_status, validate_log_level};
use super::Catalog;

/// An immutable, versioned deployment of an app.
#[derive(Debug, Clone)]
pub struct Deployment {
    pub id: String,
    pub app_id: String,
    pub version: i64,
    pub status: String,
    pub trigger_type: String,
    pub git_commit_sha: Option<String>,
    pub git_commit_message: Option<String>,
    pub build_output: Option<String>,
    pub deploy_output: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub created_at: String,
}

/// One log line belonging to a deployment.
#[derive(Debug, Clone)]
pub struct DeploymentLog {
    pub id: i64,
    pub deployment_id: String,
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

/// Validates that `value` is one of the accepted build types.
pub fn validate_build_type(value: &str) -> Result<()> {
    if matches!(value, "compose" | "dockerfile" | "nixpacks") {
        return Ok(());
    }
    bail!("build type must be one of compose, dockerfile, nixpacks");
}

/// Validates that `value` is one of the accepted source types.
pub fn validate_source_type(value: &str) -> Result<()> {
    if matches!(value, "git" | "upload") {
        return Ok(());
    }
    bail!("source type must be git or upload");
}

/// Validates that `value` is one of the accepted registry modes.
pub fn validate_registry_mode(value: &str) -> Result<()> {
    if matches!(value, "local" | "push") {
        return Ok(());
    }
    bail!("registry mode must be local or push");
}

fn read_deployment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Deployment> {
    Ok(Deployment {
        id: row.get(0)?,
        app_id: row.get(1)?,
        version: row.get(2)?,
        status: row.get(3)?,
        trigger_type: row.get(4)?,
        git_commit_sha: row.get(5)?,
        git_commit_message: row.get(6)?,
        build_output: row.get(7)?,
        deploy_output: row.get(8)?,
        started_at: row.get(9)?,
        completed_at: row.get(10)?,
        created_at: row.get(11)?,
    })
}

const DEPLOYMENT_COLUMNS: &str = "id, app_id, version, status, trigger_type, \
     git_commit_sha, git_commit_message, build_output, deploy_output, started_at, \
     completed_at, created_at";

impl Catalog {
    /// Creates a pending deployment at the next version for an app.
    pub fn create_deployment(
        &mut self,
        app_id: &str,
        trigger_type: &str,
        git_commit_sha: Option<&str>,
        git_commit_message: Option<&str>,
    ) -> Result<String> {
        if !matches!(trigger_type, "manual" | "webhook") {
            bail!("trigger type must be manual or webhook");
        }
        let id = super::new_id();
        let transaction = self.connection.transaction()?;
        let app_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM apps WHERE id = ?1)",
            params![app_id],
            |row| row.get(0),
        )?;
        if !app_exists {
            bail!("app {app_id} not found");
        }
        let version: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM deployments \
                 WHERE app_id = ?1",
            params![app_id],
            |row| row.get(0),
        )?;
        let commit_message = git_commit_message
            .map(str::trim)
            .filter(|value| !value.is_empty());
        transaction.execute(
            "INSERT INTO deployments \
                 (id, app_id, version, status, trigger_type, git_commit_sha, \
                  git_commit_message, started_at, created_at) \
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![
                id,
                app_id,
                version,
                trigger_type,
                git_commit_sha,
                commit_message
            ],
        )?;
        super::record_created(&transaction, "deployment", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Updates a deployment's status and records its output.
    ///
    /// Setting a terminal status (`running`, `failed`, `rolled_back`) also
    /// stamps `completed_at`.
    pub fn update_deployment_status(
        &mut self,
        id: &str,
        status: &str,
        output: Option<&str>,
    ) -> Result<()> {
        validate_deployment_status(status)?;
        let completed_now = matches!(status, "running" | "failed" | "rolled_back");
        let updated = self.connection.execute(
            "UPDATE deployments \
             SET status = ?2, \
                 build_output = COALESCE(?3, build_output), \
                 deploy_output = COALESCE(?3, deploy_output), \
                 completed_at = CASE WHEN ?4 THEN \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE completed_at END \
             WHERE id = ?1",
            params![id, status, output, completed_now],
        )?;
        if updated == 0 {
            bail!("deployment {id} not found");
        }
        Ok(())
    }

    /// Lists deployments for an app, newest version first.
    /// Cancels every non-terminal deployment for one app
    /// (newest push wins; the superseded builds stop mattering).
    pub fn cancel_in_flight_deployments(&mut self, app_id: &str) -> Result<u32> {
        let updated = self.connection.execute(
            "UPDATE deployments SET status = 'rolled_back', \
             completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE app_id = ?1 AND status IN ('pending', 'building', 'deploying')",
            params![app_id],
        )?;
        Ok(updated as u32)
    }

    /// Returns the in-flight deployment id for an app, if any.
    pub fn latest_in_flight_deployment(&self, app_id: &str) -> Result<Option<String>> {
        let id = self.connection.query_row(
            "SELECT id FROM deployments \
             WHERE app_id = ?1 AND status IN ('pending', 'building', 'deploying') \
             ORDER BY version DESC, rowid DESC LIMIT 1",
            params![app_id],
            |row| row.get::<_, String>(0),
        );
        match id {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn list_deployments(&self, app_id: &str) -> Result<Vec<Deployment>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {DEPLOYMENT_COLUMNS} FROM deployments \
             WHERE app_id = ?1 ORDER BY version DESC, rowid DESC"
        ))?;
        let rows = statement
            .query_map(params![app_id], read_deployment_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Lists the most recent deployments across all apps, newest
    /// first. `limit` bounds the result.
    pub fn list_recent_deployments(&self, limit: u32) -> Result<Vec<Deployment>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {DEPLOYMENT_COLUMNS} FROM deployments \
             ORDER BY started_at DESC, rowid DESC LIMIT ?1"
        ))?;
        let rows = statement
            .query_map(params![limit], read_deployment_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sets the webhook secret, enabling or disabling webhook triggers.
    ///
    /// The secret must be a non-empty opaque token; an empty secret clears it
    /// and disables the trigger.
    pub fn set_app_webhook(&mut self, id: &str, secret: Option<&str>, enabled: bool) -> Result<()> {
        if let Some(secret) = secret.map(str::trim).filter(|value| !value.is_empty()) {
            if secret.len() > 256 {
                bail!("webhook secret must be at most 256 characters");
            }
            if secret.chars().any(char::is_control) {
                bail!("webhook secret must not contain control characters");
            }
        }
        let secret = secret.map(str::trim).filter(|value| !value.is_empty());
        let updated = self.connection.execute(
            "UPDATE apps \
             SET webhook_secret = ?2, webhook_enabled = ?3, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id, secret, i64::from(enabled)],
        )?;
        if updated == 0 {
            bail!("app {id} not found");
        }
        Ok(())
    }

    /// Returns the highest-version deployment for an app.
    pub fn get_latest_deployment(&self, app_id: &str) -> Result<Option<Deployment>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {DEPLOYMENT_COLUMNS} FROM deployments \
                     WHERE app_id = ?1 ORDER BY version DESC, rowid DESC LIMIT 1"
                ),
                params![app_id],
                read_deployment_row,
            )
            .optional()
            .context("load latest deployment")
    }

    /// Loads a single deployment by id, regardless of app.
    pub fn get_deployment(&self, id: &str) -> Result<Option<Deployment>> {
        self.connection
            .query_row(
                &format!("SELECT {DEPLOYMENT_COLUMNS} FROM deployments WHERE id = ?1"),
                params![id],
                read_deployment_row,
            )
            .optional()
            .context("load deployment")
    }

    /// Appends a log line to a deployment.
    pub fn add_deployment_log(
        &mut self,
        deployment_id: &str,
        level: &str,
        message: &str,
    ) -> Result<()> {
        validate_log_level(level)?;
        self.connection.execute(
            "INSERT INTO deployment_logs (deployment_id, timestamp, level, message) \
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?2, ?3)",
            params![deployment_id, level, message],
        )?;
        Ok(())
    }

    /// Lists log lines for a deployment in chronological order.
    pub fn list_deployment_logs(&self, deployment_id: &str) -> Result<Vec<DeploymentLog>> {
        let mut statement = self.connection.prepare(
            "SELECT id, deployment_id, timestamp, level, message \
             FROM deployment_logs WHERE deployment_id = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![deployment_id], |row| {
                Ok(DeploymentLog {
                    id: row.get(0)?,
                    deployment_id: row.get(1)?,
                    timestamp: row.get(2)?,
                    level: row.get(3)?,
                    message: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
