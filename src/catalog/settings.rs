use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};

use super::Catalog;

impl Catalog {
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
}
