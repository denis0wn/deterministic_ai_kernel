use crate::providers;
use anyhow::Result;

pub fn rebuild_snapshot(_db: &str, task_id: &str, quiet: bool) -> Result<()> {
    providers::get_storage().rebuild_snapshot(task_id, quiet)
}

pub fn restore_snapshot(_db: &str, task_id: &str, quiet: bool) -> Result<()> {
    providers::get_storage().restore_snapshot(task_id, quiet)
}
