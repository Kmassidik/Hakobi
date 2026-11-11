//! Environment and deployment persistence for the CP5 flat model.
//!
//! Environments bind a build recipe to an enrolled host agent. Deployments
//! are immutable, versioned records; logs attach to a single deployment.

use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};

use super::deployments::{validate_build_type, validate_registry_mode, validate_source_type};
use super::Catalog;

/// A CP5 environment: build recipe bound to an enrolled host agent.
#[derive(Debug, Clone)]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub host_id: String,
    pub build_type: String,
    pub source_type: String,
    pub git_url: Option<String>,
    pub git_branch: Option<String>,
    pub registry_mode: String,
    pub status: String,
    pub webhook_secret: Option<String>,
    pub webhook_enabled: bool,
    pub github_repo: Option<String>,
    pub github_branch: Option<String>,
    pub github_path: Option<String>,
    pub github_dockerfile: Option<String>,
    pub github_auto_deploy: bool,
    pub github_installation_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// Validates that `value` is one of the accepted environment statuses.
pub fn validate_environment_status(value: &str) -> Result<()> {
    if matches!(
        value,
        "idle" | "building" | "deploying" | "running" | "failed"
    ) {
        return Ok(());
    }
    bail!("environment status must be one of idle, building, deploying, running, failed");
}

/// Validates that `value` is one of the accepted deployment statuses.
pub fn validate_deployment_status(value: &str) -> Result<()> {
    if matches!(
        value,
        "pending" | "building" | "deploying" | "running" | "failed" | "rolled_back"
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

/// Validates an environment name: non-empty ASCII slug, 1–64 characters.
pub fn validate_environment_name(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 64 {
        bail!("environment name must be 1-64 characters");
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        bail!("environment name must contain only ASCII letters, digits, '-', '_', or '.'");
    }
    Ok(())
}

/// Checks that `source_type` and `git_url` are consistent: git requires a
/// URL, upload requires none.
pub fn validate_source_pair(source_type: &str, git_url: Option<&str>) -> Result<()> {
    match source_type {
        "git" => {
            let url = git_url.unwrap_or_default().trim();
            if url.is_empty() {
                bail!("git URL is required when source type is git");
            }
            if url.len() > 2048 {
                bail!("git URL is too long");
            }
            Ok(())
        }
        "upload" => {
            if git_url.map(|url| !url.trim().is_empty()).unwrap_or(false) {
                bail!("git URL must be empty when source type is upload");
            }
            Ok(())
        }
        _ => validate_source_type(source_type),
    }
}

fn read_environment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Environment> {
    Ok(Environment {
        id: row.get(0)?,
        name: row.get(1)?,
        host_id: row.get(2)?,
        build_type: row.get(3)?,
        source_type: row.get(4)?,
        git_url: row.get(5)?,
        git_branch: row.get(6)?,
        registry_mode: row.get(7)?,
        status: row.get(8)?,
        webhook_secret: row.get(9)?,
        webhook_enabled: row.get::<_, i64>(10)? != 0,
        github_repo: row.get(11)?,
        github_branch: row.get(12)?,
        github_path: row.get(13)?,
        github_dockerfile: row.get(14)?,
        github_auto_deploy: row.get::<_, i64>(15)? != 0,
        github_installation_id: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

const ENVIRONMENT_COLUMNS: &str = "id, name, host_id, build_type, source_type, git_url, \
     git_branch, registry_mode, status, webhook_secret, webhook_enabled, github_repo, \
     github_branch, github_path, github_dockerfile, github_auto_deploy, github_installation_id, \
     created_at, updated_at";

impl Catalog {
    /// Creates a CP5 environment bound to an enrolled host agent.
    ///
    /// The host must exist and be enabled. The name must be a unique ASCII
    /// slug. Source/registry consistency is validated before the insert.
    #[allow(clippy::too_many_arguments)]
    pub fn create_environment(
        &mut self,
        name: &str,
        host_id: &str,
        build_type: &str,
        source_type: &str,
        git_url: Option<&str>,
        git_branch: Option<&str>,
        registry_mode: &str,
    ) -> Result<String> {
        let name = name.trim();
        validate_environment_name(name)?;
        validate_build_type(build_type)?;
        validate_source_pair(source_type, git_url)?;
        validate_registry_mode(registry_mode)?;

        let id = super::new_id();
        let transaction = self.connection.transaction()?;
        ensure_enabled_host(&transaction, host_id)?;
        if environment_name_taken(&transaction, name, None)? {
            bail!("environment {name} already exists");
        }
        transaction.execute(
            "INSERT INTO environments \
                 (id, name, host_id, build_type, source_type, git_url, git_branch, \
                  registry_mode, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'idle', \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![
                id,
                name,
                host_id,
                build_type,
                source_type,
                git_url.map(str::trim),
                git_branch.map(str::trim),
                registry_mode,
            ],
        )?;
        super::record_created(&transaction, "environment", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists CP5 environments ordered by creation.
    pub fn list_environments(&self) -> Result<Vec<Environment>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT {ENVIRONMENT_COLUMNS} FROM environments ORDER BY created_at, rowid"
        ))?;
        let rows = statement
            .query_map([], read_environment_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Loads a single CP5 environment by id.
    pub fn get_environment(&self, id: &str) -> Result<Option<Environment>> {
        self.connection
            .query_row(
                &format!("SELECT {ENVIRONMENT_COLUMNS} FROM environments WHERE id = ?1"),
                params![id],
                read_environment_row,
            )
            .optional()
            .context("load environment")
    }

    /// Replaces the mutable fields of a CP5 environment.
    #[allow(clippy::too_many_arguments)]
    pub fn update_environment(
        &mut self,
        id: &str,
        name: &str,
        host_id: &str,
        build_type: &str,
        source_type: &str,
        git_url: Option<&str>,
        git_branch: Option<&str>,
        registry_mode: &str,
    ) -> Result<()> {
        let name = name.trim();
        validate_environment_name(name)?;
        validate_build_type(build_type)?;
        validate_source_pair(source_type, git_url)?;
        validate_registry_mode(registry_mode)?;

        let transaction = self.connection.transaction()?;
        ensure_enabled_host(&transaction, host_id)?;
        if environment_name_taken(&transaction, name, Some(id))? {
            bail!("environment {name} already exists");
        }
        let updated = transaction.execute(
            "UPDATE environments \
             SET name = ?2, host_id = ?3, build_type = ?4, source_type = ?5, \
                 git_url = ?6, git_branch = ?7, registry_mode = ?8, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![
                id,
                name,
                host_id,
                build_type,
                source_type,
                git_url.map(str::trim),
                git_branch.map(str::trim),
                registry_mode,
            ],
        )?;
        if updated == 0 {
            bail!("environment {id} not found");
        }
        super::record_audit(&transaction, "update", "environment", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Sets the environment status to one of the allowed values.
    pub fn set_environment_status(&mut self, id: &str, status: &str) -> Result<()> {
        validate_environment_status(status)?;
        let updated = self.connection.execute(
            "UPDATE environments \
             SET status = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id, status],
        )?;
        if updated == 0 {
            bail!("environment {id} not found");
        }
        Ok(())
    }

    /// Permanently deletes an environment, its deployments, and their logs.
    pub fn delete_environment(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "DELETE FROM deployment_logs WHERE deployment_id IN \
                 (SELECT id FROM deployments WHERE environment_id = ?1)",
            params![id],
        )?;
        transaction.execute(
            "DELETE FROM deployments WHERE environment_id = ?1",
            params![id],
        )?;
        let deleted = transaction.execute("DELETE FROM environments WHERE id = ?1", params![id])?;
        if deleted == 0 {
            bail!("environment {id} not found");
        }
        super::record_audit(&transaction, "delete", "environment", id)?;
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

fn environment_name_taken(
    transaction: &rusqlite::Transaction<'_>,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<bool> {
    let taken: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM environments WHERE name = ?1 AND (?2 IS NULL OR id <> ?2))",
        params![name, exclude_id],
        |row| row.get(0),
    )?;
    Ok(taken)
}
