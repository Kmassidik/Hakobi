//! Stack definition CRUD.

use anyhow::{bail, Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};

use super::{new_id, record_audit, record_created, Catalog};

use crate::stacks::validate_definition;
pub use crate::stacks::{StackDefinition, StackKind};

struct StackRow {
    id: String,
    provider_id: String,
    name: String,
    kind: String,
    source: String,
    yaml: String,
    created_at: String,
    updated_at: String,
}

impl StackRow {
    fn into_definition(self) -> Result<StackDefinition> {
        let kind = self
            .kind
            .parse::<StackKind>()
            .map_err(|error: anyhow::Error| {
                error.context(format!("decode stack kind for stack {}", self.id))
            })?;
        Ok(StackDefinition {
            id: self.id,
            provider_id: self.provider_id,
            name: self.name,
            kind,
            source: self.source,
            yaml: self.yaml,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn read_stack_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StackRow> {
    Ok(StackRow {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        source: row.get(4)?,
        yaml: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn ensure_provider_exists(transaction: &Transaction<'_>, provider_id: &str) -> Result<()> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_connections WHERE id = ?1)",
        params![provider_id],
        |row| row.get(0),
    )?;
    if !exists {
        bail!("provider connection {provider_id} not found");
    }
    Ok(())
}

fn stack_name_taken(
    transaction: &Transaction<'_>,
    provider_id: &str,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<bool> {
    let taken: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM stack_definitions \
         WHERE provider_id = ?1 AND name = ?2 AND (?3 IS NULL OR id <> ?3))",
        params![provider_id, name, exclude_id],
        |row| row.get(0),
    )?;
    Ok(taken)
}

impl Catalog {
    /// Creates a stack definition for a provider and records an audit event.
    pub fn create_stack(
        &mut self,
        provider_id: &str,
        name: &str,
        kind: StackKind,
        source: &str,
        yaml: &str,
    ) -> Result<String> {
        let name = name.trim();
        if name.is_empty() {
            bail!("stack name must not be empty");
        }
        validate_definition(&kind, yaml)?;

        let id = new_id();
        let transaction = self.connection.transaction()?;
        ensure_provider_exists(&transaction, provider_id)?;
        if stack_name_taken(&transaction, provider_id, name, None)? {
            bail!("stack {name} already exists for provider {provider_id}");
        }
        transaction.execute(
            "INSERT INTO stack_definitions \
                 (id, provider_id, name, kind, source, yaml, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![id, provider_id, name, kind.as_str(), source, yaml],
        )?;
        record_created(&transaction, "stack_definition", &id)?;
        transaction.commit()?;
        Ok(id)
    }

    /// Lists stack definitions ordered by creation.
    pub fn list_stacks(&self) -> Result<Vec<StackDefinition>> {
        let mut statement = self.connection.prepare(
            "SELECT id, provider_id, name, kind, source, yaml, created_at, updated_at \
             FROM stack_definitions ORDER BY created_at, rowid",
        )?;
        let rows = statement
            .query_map([], read_stack_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(StackRow::into_definition).collect()
    }

    /// Loads a stack definition by id.
    pub fn get_stack(&self, id: &str) -> Result<Option<StackDefinition>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, provider_id, name, kind, source, yaml, created_at, updated_at \
                 FROM stack_definitions WHERE id = ?1",
                params![id],
                read_stack_row,
            )
            .optional()?;
        match row {
            Some(row) => Ok(Some(row.into_definition()?)),
            None => Ok(None),
        }
    }

    /// Replaces a stack definition and records an audit event.
    pub fn update_stack(
        &mut self,
        id: &str,
        name: &str,
        kind: StackKind,
        source: &str,
        yaml: &str,
    ) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            bail!("stack name must not be empty");
        }
        validate_definition(&kind, yaml)?;

        let transaction = self.connection.transaction()?;
        let provider_id = transaction
            .query_row(
                "SELECT provider_id FROM stack_definitions WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .with_context(|| format!("stack {id} not found"))?;
        if stack_name_taken(&transaction, &provider_id, name, Some(id))? {
            bail!("stack {name} already exists for provider {provider_id}");
        }
        transaction.execute(
            "UPDATE stack_definitions \
             SET name = ?2, kind = ?3, source = ?4, yaml = ?5, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1",
            params![id, name, kind.as_str(), source, yaml],
        )?;
        record_audit(&transaction, "update", "stack_definition", id)?;
        transaction.commit()?;
        Ok(())
    }

    /// Permanently deletes a stack definition and records an audit event.
    pub fn delete_stack(&mut self, id: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let deleted =
            transaction.execute("DELETE FROM stack_definitions WHERE id = ?1", params![id])?;
        if deleted == 0 {
            bail!("stack {id} not found");
        }
        record_audit(&transaction, "delete", "stack_definition", id)?;
        transaction.commit()?;
        Ok(())
    }
}
