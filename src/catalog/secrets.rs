use std::collections::{BTreeMap, HashMap};

use anyhow::{bail, Result};
use rusqlite::params;

use super::{App, Catalog};

pub fn validate_secret_name(name: &str) -> Result<()> {
    let mut characters = name.chars();
    if name.is_empty()
        || name.len() > 255
        || !characters
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !characters.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        bail!("secret names must be valid environment variable names");
    }
    Ok(())
}

impl Catalog {
    pub fn list_app_secret_names(&self, app_id: &str) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT name FROM app_secrets WHERE app_id = ?1 ORDER BY name")?;
        let rows = statement.query_map(params![app_id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_app_secrets(
        &mut self,
        app_id: &str,
        secrets: &BTreeMap<String, String>,
    ) -> Result<()> {
        let existing = self.list_app_secret_names(app_id)?;
        let mut encrypted = BTreeMap::new();
        for (name, value) in secrets {
            validate_secret_name(name)?;
            if value.is_empty() {
                if !existing.contains(name) {
                    bail!("new secret {name} must have a value");
                }
            } else {
                encrypted.insert(name, self.encrypt_secret(value)?);
            }
        }
        let transaction = self.connection.transaction()?;
        for name in existing {
            if !secrets.contains_key(&name) {
                transaction.execute(
                    "DELETE FROM app_secrets WHERE app_id = ?1 AND name = ?2",
                    params![app_id, name],
                )?;
            }
        }
        for (name, value) in encrypted {
            transaction.execute(
                "INSERT INTO app_secrets (app_id, name, encrypted_value) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(app_id, name) DO UPDATE SET encrypted_value = excluded.encrypted_value",
                params![app_id, name, value],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn app_environment(&self, app: &App) -> Result<HashMap<String, String>> {
        let mut environment: HashMap<String, String> = serde_json::from_str(&app.env_json)?;
        let mut statement = self.connection.prepare(
            "SELECT name, encrypted_value FROM app_secrets WHERE app_id = ?1 ORDER BY name",
        )?;
        let rows = statement.query_map(params![&app.id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (name, encrypted) = row?;
            environment.insert(name, self.decrypt_secret(&encrypted)?);
        }
        Ok(environment)
    }
}
