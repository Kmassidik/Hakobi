//! Host agent CRUD.

use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};

use super::{new_id, record_audit, record_created, Catalog};

/// A host agent registered in the catalog.
///
/// The encrypted token is intentionally absent from this view.
#[derive(Debug, Clone)]
pub struct HostAgent {
    pub id: String,
    pub name: String,
    pub agent_url: String,
    pub enabled: bool,
    pub last_checked_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

impl Catalog {
    /// Creates a host agent and its mapped provider row atomically.
    ///
    /// This is a convenience wrapper around [`Catalog::create_host_agent`] and
    /// [`Catalog::create_host_agent_provider`] that performs both inserts in a
    /// single transaction so they commit or roll back together.
    pub fn create_host_agent_with_provider(
        &mut self,
        name: &str,
        agent_url: &str,
        token: &str,
    ) -> Result<String> {
        let id = new_id();
        let encrypted_token = self.encrypt_secret(token)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO host_agents \
                 (id, name, agent_url, encrypted_token, enabled, created_at) \
             VALUES (?1, ?2, ?3, ?4, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, name, agent_url, encrypted_token],
        )?;
        record_created(&transaction, "host_agent", &id)?;

        // Derive the provider row from the just-created host agent.
        let provider_name = format!("{name} (agent)");
        let endpoint = format!("agent://{id}");
        let provider_id = new_id();
        transaction.execute(
            "INSERT INTO provider_connections \
                 (id, name, kind, endpoint, encrypted_secret, capabilities, enabled, created_at) \
             VALUES (?1, ?2, ?3, ?4, NULL, '[]', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![provider_id, provider_name, "docker", endpoint],
        )?;
        record_created(&transaction, "provider_connection", &provider_id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Creates a host agent and records an audit event.
    pub fn create_host_agent(
        &mut self,
        name: &str,
        agent_url: &str,
        token: &str,
    ) -> Result<String> {
        let id = new_id();
        let encrypted_token = self.encrypt_secret(token)?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO host_agents \
                 (id, name, agent_url, encrypted_token, enabled, created_at) \
             VALUES (?1, ?2, ?3, ?4, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, name, agent_url, encrypted_token],
        )?;
        record_created(&transaction, "host_agent", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists host agents without revealing credentials.
    pub fn list_host_agents(&self) -> Result<Vec<HostAgent>> {
        let mut statement = self.connection.prepare(
            "SELECT id, name, agent_url, enabled, last_checked_at, last_error, created_at \
             FROM host_agents ORDER BY created_at, rowid",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(
                |(id, name, agent_url, enabled, last_checked_at, last_error, created_at)| {
                    Ok(HostAgent {
                        id,
                        name,
                        agent_url,
                        enabled,
                        last_checked_at,
                        last_error,
                        created_at,
                    })
                },
            )
            .collect()
    }

    /// Returns the decrypted token for a host agent.
    pub fn host_agent_token(&self, id: &str) -> Result<String> {
        let encrypted_token = self
            .connection
            .query_row(
                "SELECT encrypted_token FROM host_agents WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .with_context(|| format!("host agent {id} not found"))?
            .with_context(|| format!("host agent {id} has no stored token"))?;
        self.decrypt_secret(&encrypted_token)
    }

    /// Records the latest health check for a host agent.
    pub fn record_host_agent_health(&mut self, id: &str, error: Option<&str>) -> Result<()> {
        let updated = self.connection.execute(
            "UPDATE host_agents \
             SET last_checked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), last_error = ?2 \
             WHERE id = ?1",
            params![id, error],
        )?;
        if updated == 0 {
            bail!("host agent {id} not found");
        }
        Ok(())
    }

    /// Enables or disables a host agent without discarding its token.
    pub fn set_host_agent_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE host_agents SET enabled = ?2 WHERE id = ?1",
            params![id, i64::from(enabled)],
        )?;
        if updated == 0 {
            bail!("host agent {id} not found");
        }
        record_audit(
            &transaction,
            if enabled { "enable" } else { "disable" },
            "host_agent",
            id,
        )?;
        transaction.execute(
            "UPDATE provider_connections SET enabled = ?2 WHERE endpoint = ?1",
            params![format!("agent://{id}"), i64::from(enabled)],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Permanently deletes a host agent and its mapped provider row atomically.
    pub fn delete_host_agent(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM host_agents WHERE id = ?1",
                params![id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            bail!("host agent {id} not found");
        }
        transaction.execute(
            "DELETE FROM provider_connections WHERE endpoint = ?1",
            params![format!("agent://{id}")],
        )?;
        transaction.execute("DELETE FROM host_agents WHERE id = ?1", params![id])?;
        record_audit(&transaction, "delete", "host_agent", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Creates a `docker` provider row mapped to a host agent, or returns the
    /// existing provider ID if one already exists for this agent URL.
    ///
    /// This is a convenience wrapper around [`Catalog::create_provider_connection`]
    /// that derives the provider name and endpoint from the host agent.
    pub fn create_host_agent_provider(&mut self, host_agent_id: &str) -> Result<String> {
        // Read the agent to derive endpoint/name; single-row query is cheap.
        let (name, _agent_url) = self
            .connection
            .query_row(
                "SELECT name, agent_url FROM host_agents WHERE id = ?1",
                params![host_agent_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .with_context(|| format!("host agent {host_agent_id} not found"))?;

        let provider_name = format!("{name} (agent)");
        let endpoint = format!("agent://{host_agent_id}");

        // Check if a provider for this agent already exists.
        let existing = self
            .connection
            .query_row(
                "SELECT id FROM provider_connections WHERE endpoint = ?1",
                params![endpoint],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        if let Some(provider_id) = existing {
            return Ok(provider_id);
        }

        let id = self.create_provider_connection(&provider_name, "docker", &endpoint, "")?;
        Ok(id)
    }

    /// Deletes the `docker` provider row mapped to a host agent.
    pub fn delete_host_agent_provider(&mut self, host_agent_id: &str) -> Result<()> {
        let endpoint = format!("agent://{host_agent_id}");
        let transaction = self.connection.transaction()?;
        let deleted = transaction.execute(
            "DELETE FROM provider_connections WHERE endpoint = ?1",
            params![endpoint],
        )?;
        if deleted == 0 {
            // Provider may have been deleted separately; not an error.
        }
        transaction.commit()?;
        Ok(())
    }
}
