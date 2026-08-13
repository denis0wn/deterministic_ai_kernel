use crate::providers::storage::StorageProvider;
use anyhow::Result;
use std::collections::BTreeMap;

// All scheduler facades take an explicit database path and construct a
// storage instance bound to it. No process/thread-global routing state is
// touched (audit finding M3).

pub fn seed_dependencies(db: &str, task_id: &str) -> Result<()> {
    crate::providers::storage_for(db).seed_dependencies(task_id)
}

pub fn reconcile(db: &str, task_id: &str) -> Result<()> {
    let _ = crate::providers::storage_for(db).reconcile_scheduler(task_id)?;
    Ok(())
}

pub fn schedule(db: &str, task_id: &str) -> Result<()> {
    let _ = crate::providers::storage_for(db).schedule_next_steps(task_id)?;
    Ok(())
}

pub fn current_status_map(db: &str, task_id: &str) -> Result<BTreeMap<String, String>> {
    crate::providers::storage_for(db).get_current_status_map(task_id)
}

pub fn next_ready_step(db: &str, task_id: &str) -> Result<Option<String>> {
    crate::providers::storage_for(db).get_next_ready_step(task_id)
}

#[allow(dead_code)]
pub(crate) fn unlock_ready_steps_by_db(db: &str, task_id: &str) -> Result<()> {
    crate::providers::storage_for(db).unlock_ready_steps_by_db(task_id)
}
