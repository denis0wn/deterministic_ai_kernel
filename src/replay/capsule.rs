#![allow(dead_code, unused)]
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

use crate::event_bus::EventBus;
use crate::kernel_types::{ReplayCapsule, TrustContext, TrustLevel};
use crate::workflow::pipeline::PipelineOutput;

pub fn build_replay_capsule(bus: &EventBus, task_id: &str) -> Result<ReplayCapsule> {
    build_replay_capsule_inner(bus, task_id, None)
}

pub fn build_replay_capsule_from_pipeline(
    bus: &EventBus,
    output: &PipelineOutput,
) -> Result<ReplayCapsule> {
    build_replay_capsule_inner(bus, &output.task_id, Some(output))
}

fn build_replay_capsule_inner(
    bus: &EventBus,
    task_id: &str,
    pipeline: Option<&PipelineOutput>,
) -> Result<ReplayCapsule> {
    let events = bus.list_execution_events(task_id)?;
    let state_graph = bus.build_state_graph(task_id)?;

    let event_ids = events.iter().map(|e| e.id.clone()).collect::<Vec<_>>();

    let mut environment: BTreeMap<String, String> = BTreeMap::from([
        ("source".to_string(), "event_bus".to_string()),
        ("capture_mode".to_string(), "v1".to_string()),
    ]);

    let determinism_envelope = if let Some(p) = pipeline {
        let stable_ids: Vec<String> = p.steps.iter().map(|s| s.id.to_string()).collect();
        environment.insert("seed".to_string(), p.seed.to_string());
        environment.insert(
            "planner_version".to_string(),
            p.manifest.planner_version.clone(),
        );
        json!({
            "inside": ["event_log", "state_graph", "captured_payloads", "seed", "stable_ids"],
            "outside": ["wall_clock", "external_services", "human_input"],
            "seed": p.seed,
            "planner_version": p.manifest.planner_version,
            "stable_ids": stable_ids,
        })
    } else {
        json!({
            "inside": ["event_log", "state_graph", "captured_payloads"],
            "outside": ["wall_clock", "external_services", "human_input"]
        })
    };

    Ok(ReplayCapsule {
        capsule_id: format!("capsule-{}", task_id),
        execution_id: task_id.to_string(),
        created_at: "now".to_string(),
        state_graph,
        event_ids,
        artifacts: vec![],
        environment,
        decision_points: vec![],
        determinism_envelope,
        trust_context: TrustContext {
            source: "replay_capsule_builder".into(),
            trust_level: TrustLevel::High,
            verification_status: "derived_from_event_log".into(),
            policy_version: "v1".into(),
        },
    })
}
