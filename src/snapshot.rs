use crate::providers;
use crate::providers::storage::StorageProvider;
use anyhow::Result;

// Snapshot operations honor their database argument (the previous
// implementation ignored `_db` and relied on leaked global routing state —
// audit findings M3/H1).

pub fn rebuild_snapshot(db: &str, task_id: &str, quiet: bool) -> Result<()> {
    providers::storage_for(db).rebuild_snapshot(task_id, quiet)
}

pub fn restore_snapshot(db: &str, task_id: &str, quiet: bool) -> Result<()> {
    providers::storage_for(db).restore_snapshot(task_id, quiet)
}
