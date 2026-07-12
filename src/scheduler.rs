use anyhow::Result;
use std::collections::BTreeMap;

pub fn seed_dependencies(db: &str, task_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().seed_dependencies(task_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

pub fn reconcile(db: &str, task_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let _ = crate::providers::get_storage().reconcile_scheduler(task_id)?;
    crate::providers::get_storage().set_override_path(None);
    Ok(())
}

pub fn schedule(db: &str, task_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let _ = crate::providers::get_storage().schedule_next_steps(task_id)?;
    crate::providers::get_storage().set_override_path(None);
    Ok(())
}

pub fn current_status_map(db: &str, task_id: &str) -> Result<BTreeMap<String, String>> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().get_current_status_map(task_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

pub fn next_ready_step(db: &str, task_id: &str) -> Result<Option<String>> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().get_next_ready_step(task_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

#[allow(dead_code)]
pub(crate) fn unlock_ready_steps_by_db(db: &str, task_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().unlock_ready_steps_by_db(task_id);
    crate::providers::get_storage().set_override_path(None);
    res
}
