use crate::providers::storage::StorageProvider;
pub fn replay_validate(db: &str, task_id: &str) -> bool {
    if !std::path::Path::new(db).exists() {
        // Missing database: no events to validate. Treat as valid (nothing to invalidate).
        // This is a design decision — allows reconstruct_state to proceed for new tasks.
        tracing::warn!(
            db = %db,
            task_id = %task_id,
            "replay_validate: database not found, treating as valid (no events to validate)"
        );
        return true;
    }
    crate::providers::storage_for(db).replay_validate(task_id)
}
