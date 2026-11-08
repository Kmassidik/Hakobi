//! Legacy CP1 organization/project/environment/service hierarchy.
//!
//! The hierarchy is unused by the CP5 flat model but is kept intact so
//! existing data and tests keep working.

use anyhow::Result;
use rusqlite::params;

use super::{new_id, record_created, Catalog};

/// A service registered in the catalog.
#[derive(Debug, Clone)]
pub struct Service {
    pub id: String,
    pub environment_id: String,
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

/// An immutable record of a catalog mutation.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub id: String,
    pub action: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub actor: String,
    pub created_at: String,
}

impl Catalog {
    /// Creates an organization and records an audit event.
    pub fn create_organization(&mut self, slug: &str, name: &str) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO organizations (id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, slug, name],
        )?;
        record_created(&transaction, "organization", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Creates a project inside an organization and records an audit event.
    pub fn create_project(
        &mut self,
        organization_id: &str,
        slug: &str,
        name: &str,
    ) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO projects (id, organization_id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, organization_id, slug, name],
        )?;
        record_created(&transaction, "project", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Creates an environment inside a project (legacy CP1 hierarchy).
    ///
    /// Superseded by the flat CP5 [`Catalog::create_environment`]; kept for
    /// backward compatibility with existing data.
    pub fn create_project_environment(
        &mut self,
        project_id: &str,
        slug: &str,
        name: &str,
    ) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO legacy_environments (id, project_id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, project_id, slug, name],
        )?;
        record_created(&transaction, "environment", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Creates a service inside an environment and records an audit event.
    pub fn create_service(
        &mut self,
        environment_id: &str,
        slug: &str,
        name: &str,
    ) -> Result<String> {
        let id = new_id();
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO services (id, environment_id, slug, name, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, environment_id, slug, name],
        )?;
        record_created(&transaction, "service", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists services ordered by creation.
    pub fn list_services(&self) -> Result<Vec<Service>> {
        let mut statement = self.connection.prepare(
            "SELECT id, environment_id, slug, name, created_at \
             FROM services ORDER BY created_at, rowid",
        )?;
        let services = statement
            .query_map([], |row| {
                Ok(Service {
                    id: row.get(0)?,
                    environment_id: row.get(1)?,
                    slug: row.get(2)?,
                    name: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(services)
    }

    /// Lists audit events ordered by creation.
    pub fn list_audit_events(&self) -> Result<Vec<AuditEvent>> {
        let mut statement = self.connection.prepare(
            "SELECT id, action, entity_kind, entity_id, actor, created_at \
             FROM audit_events ORDER BY created_at, rowid",
        )?;
        let events = statement
            .query_map([], |row| {
                Ok(AuditEvent {
                    id: row.get(0)?,
                    action: row.get(1)?,
                    entity_kind: row.get(2)?,
                    entity_id: row.get(3)?,
                    actor: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }
}
