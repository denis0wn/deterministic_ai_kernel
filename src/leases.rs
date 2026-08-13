use crate::providers::storage::StorageProvider;
use anyhow::Result;

// Explicit database routing; no global override state (audit finding M3).

pub fn seed_demo_leases(db: &str, task_id: &str) -> Result<()> {
    crate::providers::storage_for(db).seed_demo_leases(task_id)
}

pub fn expire_leases(db: &str, task_id: &str) -> Result<()> {
    crate::providers::storage_for(db).expire_leases(task_id)
}
