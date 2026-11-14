//! Persistent catalog for apps, hosts, providers, and deployments.
//!
//! The catalog is a SQLite database. Its schema is versioned with
//! `PRAGMA user_version`; opening an up-to-date database is a no-op.

mod apps;
mod audit;
mod deployments;
mod github;
mod hosts;
mod migrations;
mod providers;
mod settings;
mod vault;

/// Upper bound for [`Catalog::list_recent_deployments`].
pub const DEPLOYMENT_PAGE_LIMIT: u32 = 100;

pub use apps::{
    validate_app_name, validate_app_status, validate_deployment_status, validate_log_level, App,
};
pub use deployments::{
    validate_build_type, validate_registry_mode, validate_source_type, Deployment, DeploymentLog,
};
pub use github::{GithubApp, GithubConnection};
pub use hosts::HostAgent;
pub use providers::ProviderConnection;

use audit::{new_id, record_audit, record_created};

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::Connection;
use vault::{load_or_create_key, KEY_LEN};

/// Capabilities a provider connection may be granted.
const CAPABILITIES: [&str; 3] = ["read", "operate", "admin"];

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

    fn migrate(&mut self) -> Result<()> {
        migrations::migrate(&mut self.connection)
    }
}
