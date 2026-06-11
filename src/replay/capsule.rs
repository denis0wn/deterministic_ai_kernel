use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

use crate::event_bus::EventBus;
use crate::kernel_types::{ReplayCapsule, TrustContext, TrustLevel};

pub fn build_replay_capsule(bus: &EventBus, task_id: &str) -> Result<ReplayCapsule> {
    let events = bus.list_execution_events(task_id)?;
    let state_graph = bus.build_state_graph(task_id)?;

    let event_ids = events.iter().map(|e| e.id.clone()).collect::<Vec<_>>();

    Ok(ReplayCapsule {
        capsule_id: format!("capsule-{}", task_id),
        execution_id: task_id.to_string(),
        created_at: "now".to_string(),
        state_graph,
        event_ids,
        artifacts: vec![],
        environment: BTreeMap::from([
            ("source".to_string(), "event_bus".to_string()),
            ("capture_mode".to_string(), "v0".to_string()),
        ]),
        decision_points: vec![],
        determinism_envelope: json!({
            "inside": ["event_log", "state_graph", "captured_payloads"],
            "outside": ["wall_clock", "external_services", "human_input"]
        }),
        trust_context: TrustContext {
            source: "replay_capsule_builder".into(),
            trust_level: TrustLevel::High,
            verification_status: "derived_from_event_log".into(),
            policy_version: "v1".into(),
        },
    })
}
