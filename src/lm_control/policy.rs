use anyhow::Result;

use crate::model_manifest;

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchPlan {
    pub model: String,
    pub ram_class: String,
    pub threshold_gb: f64,
    pub free_gb: f64,
}

pub fn switch_plan(role: &str, free_gb: f64) -> Result<SwitchPlan> {
    let selected = model_manifest::best_enabled_model_for_role(role)?;
    let threshold_gb = model_manifest::threshold_gb_for_ram_class(&selected.ram_class)?;
    Ok(SwitchPlan {
        model: selected.id,
        ram_class: selected.ram_class,
        threshold_gb,
        free_gb,
    })
}
