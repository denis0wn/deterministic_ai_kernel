#![allow(dead_code)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::lm_control;
use crate::model_manifest;

pub mod lm_studio_client;
pub mod policy_applier;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelSelectionPolicy {
    pub roles: Vec<ModelRolePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelRolePolicy {
    pub role: String,
    pub selected_model: String,
    pub priority: u32,
    pub enabled: bool,
    pub ram_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RamGatingPolicy {
    pub thresholds_gb: Vec<RamThresholdPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RamThresholdPolicy {
    pub ram_class: String,
    pub threshold_gb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannerHeuristicsPolicy {
    pub fallback_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvSyncPolicy {
    pub managed_roles: Vec<EnvRolePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvRolePolicy {
    pub role: String,
    pub env_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyState {
    pub model_selection_policy: ModelSelectionPolicy,
    pub ram_gating_policy: RamGatingPolicy,
    pub planner_heuristics_policy: PlannerHeuristicsPolicy,
    pub env_sync_policy: EnvSyncPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyContext {
    pub system_state: Value,
    pub metrics: Value,
    pub recent_failures: Value,
    pub policy_surfaces: PolicyState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyUpdate {
    pub surface: String,
    pub key: String,
    pub old: Value,
    pub new: Value,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyUpdateEnvelope {
    pub parameter_updates: Vec<PolicyUpdate>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyVersionRecord {
    pub version_id: String,
    pub created_at_unix: u64,
    pub confidence: f64,
    pub snapshot: PolicyState,
    pub diff: Vec<PolicyUpdate>,
    pub rollback_pointer: Option<String>,
}

pub fn collect_policy_state() -> Result<PolicyState> {
    let manifest = model_manifest::load_manifest()?;
    let mut roles = manifest
        .models
        .iter()
        .map(|m| ModelRolePolicy {
            role: m.role.clone(),
            selected_model: m.id.clone(),
            priority: m.priority,
            enabled: m.enabled,
            ram_class: m.ram_class.clone(),
        })
        .collect::<Vec<_>>();
    roles.sort_by(|a, b| a.role.cmp(&b.role).then(a.priority.cmp(&b.priority)));

    let thresholds_gb = ["light", "medium", "heavy"]
        .into_iter()
        .map(|ram_class| {
            Ok(RamThresholdPolicy {
                ram_class: ram_class.to_string(),
                threshold_gb: model_manifest::threshold_gb_for_ram_class(ram_class)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let managed_roles = ["coding_assistant", "task_planning", "embeddings"]
        .into_iter()
        .map(|role| {
            Ok(EnvRolePolicy {
                role: role.to_string(),
                env_key: model_manifest::env_key_for_role(role)?.to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(PolicyState {
        model_selection_policy: ModelSelectionPolicy { roles },
        ram_gating_policy: RamGatingPolicy { thresholds_gb },
        planner_heuristics_policy: PlannerHeuristicsPolicy {
            fallback_keywords: vec!["fallback".into(), "error".into()],
        },
        env_sync_policy: EnvSyncPolicy { managed_roles },
    })
}

pub fn build_policy_context() -> Result<PolicyContext> {
    let policy_surfaces = collect_policy_state()?;
    let free_gb = lm_control::free_memory_gb_estimate()?;
    let models = lm_control::list_models().unwrap_or_default();
    let model_statuses = model_manifest::current_model_statuses()?;

    Ok(PolicyContext {
        system_state: json!({
            "lm_backend": std::env::var("DAK_LM_BACKEND").unwrap_or_else(|_| "lm_studio".to_string()),
            "local_models": models,
            "env_sync_status": model_statuses,
        }),
        metrics: json!({
            "free_memory_gb": free_gb,
            "policy_surface_count": 4,
        }),
        recent_failures: json!([]),
        policy_surfaces,
    })
}

pub fn version_dir() -> PathBuf {
    PathBuf::from("policy_versions")
}

pub fn write_policy_version(record: &PolicyVersionRecord) -> Result<PathBuf> {
    let dir = version_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", record.version_id));
    fs::write(&path, serde_json::to_vec_pretty(record)?)?;
    Ok(path)
}

pub fn latest_policy_version() -> Result<Option<PolicyVersionRecord>> {
    let dir = version_dir();
    if !dir.exists() {
        return Ok(None);
    }

    let mut entries = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    entries.sort();

    let Some(last) = entries.last() else {
        return Ok(None);
    };
    let text = fs::read_to_string(last)?;
    Ok(Some(serde_json::from_str(&text)?))
}

pub fn new_version_id() -> Result<String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow!("system clock before unix epoch"))?
        .as_secs();
    Ok(format!("policy-{}", ts))
}

pub fn ensure_surface_is_allowed(surface: &str) -> Result<()> {
    match surface {
        "model_selection" | "ram_gating" | "planner" | "env_sync" => Ok(()),
        other => Err(anyhow!("unsupported policy surface: {}", other)),
    }
}

pub fn policy_file_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_state_has_four_surfaces() {
        let state = collect_policy_state().unwrap();
        assert!(!state.model_selection_policy.roles.is_empty());
        assert_eq!(state.ram_gating_policy.thresholds_gb.len(), 3);
        assert_eq!(state.planner_heuristics_policy.fallback_keywords.len(), 2);
        assert_eq!(state.env_sync_policy.managed_roles.len(), 3);
    }

    #[test]
    fn allowed_surfaces_are_strict() {
        ensure_surface_is_allowed("model_selection").unwrap();
        ensure_surface_is_allowed("ram_gating").unwrap();
        ensure_surface_is_allowed("planner").unwrap();
        ensure_surface_is_allowed("env_sync").unwrap();
        assert!(ensure_surface_is_allowed("execution_core").is_err());
    }
}
