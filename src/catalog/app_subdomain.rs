//! Optional public DNS label persistence for tunnel-published applications.

use anyhow::{bail, Result};
use rusqlite::params;

use super::Catalog;

pub fn validate_app_subdomain(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let valid = (1..=63).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && (bytes[bytes.len() - 1].is_ascii_lowercase() || bytes[bytes.len() - 1].is_ascii_digit())
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if !valid {
        bail!("subdomain must be a lowercase DNS label of 1-63 characters");
    }
    Ok(())
}

impl Catalog {
    /// Sets or clears the public DNS label for an app.
    pub fn set_app_subdomain(&self, id: &str, subdomain: Option<&str>) -> Result<()> {
        if let Some(value) = subdomain {
            validate_app_subdomain(value)?;
        }
        let updated = self.connection.execute(
            "UPDATE apps \
             SET subdomain = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id, subdomain],
        )?;
        if updated == 0 {
            bail!("app {id} not found");
        }
        Ok(())
    }
}
