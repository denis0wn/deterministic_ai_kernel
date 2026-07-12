use anyhow::Result;

pub fn seed_demo_leases(db: &str, task_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().seed_demo_leases(task_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

pub fn expire_leases(db: &str, task_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().expire_leases(task_id);
    crate::providers::get_storage().set_override_path(None);
    res
}
