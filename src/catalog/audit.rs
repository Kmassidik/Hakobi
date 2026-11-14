use anyhow::Result;
use rusqlite::{params, Transaction};
use uuid::Uuid;

pub(super) fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub(super) fn record_audit(
    transaction: &Transaction<'_>,
    action: &str,
    entity_kind: &str,
    entity_id: &str,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO audit_events (id, action, entity_kind, entity_id, actor, created_at) \
         VALUES (?1, ?2, ?3, ?4, 'system', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        params![new_id(), action, entity_kind, entity_id],
    )?;
    Ok(())
}

pub(super) fn record_created(
    transaction: &Transaction<'_>,
    entity_kind: &str,
    entity_id: &str,
) -> Result<()> {
    record_audit(transaction, "create", entity_kind, entity_id)
}
