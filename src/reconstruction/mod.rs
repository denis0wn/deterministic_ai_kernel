use crate::event_bus::EventBus;
use crate::kernel_types::ReplayCapsule;
use crate::providers::storage::StorageProvider;
use crate::workflow::contract::WorkerCapability;
use anyhow::Result;
use std::collections::HashMap;

pub type ReconstructionCapsule = ReplayCapsule;

fn is_ai_worker(worker_id: &str) -> bool {
    let lower = worker_id.to_ascii_lowercase();
    lower == "ai"
        || lower.contains("worker-ai")
        || lower.contains("ai-worker")
        || lower.starts_with("ai-")
        || lower.starts_with("ai_")
}

pub(crate) fn capability_for_worker_id(worker_id: &str) -> Result<WorkerCapability> {
    let lower = worker_id.to_ascii_lowercase();

    if is_ai_worker(worker_id) {
        return Ok(WorkerCapability::LegacyGeneric);
    }

    if lower.contains("planner") {
        return Ok(WorkerCapability::Planner);
    }

    if lower.contains("executor") {
        return Ok(WorkerCapability::Executor);
    }

    if lower.contains("verifier") {
        return Ok(WorkerCapability::Verifier);
    }

    if lower.starts_with("worker-") || lower.starts_with("worker_") {
        return Ok(WorkerCapability::LegacyGeneric);
    }

    Err(anyhow::anyhow!(
        "worker has no declared capability: {}",
        worker_id
    ))
}

pub fn reconstruct_state(db: &str, task_id: &str) -> bool {
    let mut ok = true;

    if !crate::replay::engine::replay_validate(db, task_id) {
        eprintln!("RECONSTRUCTION INVALID: replay validation failed");
        ok = false;
    }

    if !std::path::Path::new(db).exists() {
        // No database: nothing to reconstruct. Return current validation status.
        return ok;
    }

    let storage = crate::providers::storage_for(db);

    let spec = match storage.load_exec_spec(task_id) {
        Ok(s) => s,
        Err(_) => {
            return ok;
        }
    };

    let calculated_hash = spec.calculate_hash();
    if spec.spec_id != calculated_hash {
        eprintln!(
            "RECONSTRUCTION INVALID: ExecSpec hash inconsistency. spec_id={}, calculated={}",
            spec.spec_id, calculated_hash
        );
        ok = false;
    }

    let rows = match storage.list_event_log(task_id) {
        Ok(r) => r,
        Err(_) => {
            return false;
        }
    };

    let mut completed_steps = HashMap::new();
    let mut started_steps = Vec::new();

    for row in rows {
        let (generation, step_id_opt, event_type, payload_str) = row;
        let step_id = step_id_opt.unwrap_or_default();

        match event_type.as_str() {
            "STEP_COMPLETED" | "PrimitiveCompleted" => {
                completed_steps.insert(step_id, generation);
            }
            "STEP_STARTED" | "PrimitiveStarted" => {
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&payload_str) {
                    if let Some(worker_id) = payload.get("worker_id").and_then(|w| w.as_str()) {
                        started_steps.push((step_id, worker_id.to_string()));
                    }
                }
            }
            _ => {}
        }
    }

    for dep in &spec.dependencies {
        if let Some(&dep_gen) = completed_steps.get(&dep.step_id) {
            for parent_id in &dep.depends_on {
                match completed_steps.get(parent_id) {
                    Some(&parent_gen) => {
                        if parent_gen >= dep_gen {
                            eprintln!(
                                "RECONSTRUCTION INVALID: dependency violation. {} (gen {}) completed before parent {} (gen {})",
                                dep.step_id, dep_gen, parent_id, parent_gen
                            );
                            ok = false;
                        }
                    }
                    None => {
                        eprintln!(
                            "RECONSTRUCTION INVALID: dependency violation. {} completed, but parent {} is not completed",
                            dep.step_id, parent_id
                        );
                        ok = false;
                    }
                }
            }
        }
    }

    for (step_id, worker_id) in &started_steps {
        if let Some(step_spec) = spec.steps.iter().find(|s| s.step_id == *step_id) {
            let cap_constraint = step_spec
                .constraints
                .iter()
                .find(|c| c.key == "required_capability");
            if let Some(c) = cap_constraint {
                let required_capability = match c.value.as_str() {
                    "Planner" => Some(WorkerCapability::Planner),
                    "Executor" => Some(WorkerCapability::Executor),
                    "Verifier" => Some(WorkerCapability::Verifier),
                    "LegacyGeneric" => Some(WorkerCapability::LegacyGeneric),
                    _ => None,
                };

                if let Some(req_cap) = required_capability {
                    if let Ok(worker_cap) = capability_for_worker_id(worker_id) {
                        if worker_cap != req_cap && worker_cap != WorkerCapability::LegacyGeneric {
                            eprintln!(
                                "RECONSTRUCTION INVALID: capability constraint violation for step {}. worker {} has capability {:?}, but step requires {:?}",
                                step_id, worker_id, worker_cap, req_cap
                            );
                            ok = false;
                        }
                    }
                }
            }
        }
    }

    ok
}

pub fn build_reconstruction_capsule(
    bus: &EventBus,
    task_id: &str,
) -> Result<ReconstructionCapsule> {
    crate::replay::capsule::build_replay_capsule(bus, task_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_for_worker_id_planner() {
        assert_eq!(
            capability_for_worker_id("worker-planner").unwrap(),
            WorkerCapability::Planner
        );
        assert_eq!(
            capability_for_worker_id("my-planner-1").unwrap(),
            WorkerCapability::Planner
        );
    }

    #[test]
    fn capability_for_worker_id_executor() {
        assert_eq!(
            capability_for_worker_id("worker-executor").unwrap(),
            WorkerCapability::Executor
        );
    }

    #[test]
    fn capability_for_worker_id_verifier() {
        assert_eq!(
            capability_for_worker_id("worker-verifier").unwrap(),
            WorkerCapability::Verifier
        );
    }

    #[test]
    fn capability_for_worker_id_legacy_generic() {
        assert_eq!(
            capability_for_worker_id("worker-ai").unwrap(),
            WorkerCapability::LegacyGeneric
        );
        assert_eq!(
            capability_for_worker_id("ai").unwrap(),
            WorkerCapability::LegacyGeneric
        );
        assert_eq!(
            capability_for_worker_id("worker-anything").unwrap(),
            WorkerCapability::LegacyGeneric
        );
    }

    #[test]
    fn capability_for_worker_id_rejects_unknown() {
        assert!(capability_for_worker_id("unknown").is_err());
    }

    #[test]
    fn is_ai_worker_matches_patterns() {
        assert!(is_ai_worker("ai"));
        assert!(is_ai_worker("worker-ai"));
        assert!(is_ai_worker("ai-worker"));
        assert!(is_ai_worker("ai-assistant"));
        assert!(is_ai_worker("ai_helper"));
        assert!(!is_ai_worker("worker-planner"));
        assert!(!is_ai_worker("worker-executor"));
    }
}
