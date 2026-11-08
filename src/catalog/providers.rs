//! Provider connection CRUD.

use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension};

use super::{new_id, record_audit, record_created, Catalog, CAPABILITIES};

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

impl Catalog {
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
}
