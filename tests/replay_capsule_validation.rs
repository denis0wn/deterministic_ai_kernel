use deterministic_ai_kernel::kernel_types::{
    ReplayCapsule, StateGraph, TrustContext, TrustLevel,
};
use serde_json::json;
use std::collections::BTreeMap;

fn base_capsule() -> ReplayCapsule {
    ReplayCapsule {
        capsule_id: "capsule-1".into(),
        execution_id: "task-1".into(),
        created_at: "now".into(),
        state_graph: StateGraph {
            nodes: vec![deterministic_ai_kernel::kernel_types::StateGraphNode {
                id: "node-evt-1".into(),
                kind: "event".into(),
                ref_id: "evt-1".into(),
            }],
            edges: vec![],
        },
        event_ids: vec!["evt-1".into()],
        artifacts: vec![],
        environment: BTreeMap::from([("source".into(), "event_bus".into())]),
        decision_points: vec![],
        determinism_envelope: json!({"inside":["event_log"],"outside":["wall_clock"]}),
        trust_context: TrustContext {
            source: "test".into(),
            trust_level: TrustLevel::High,
            verification_status: "test".into(),
            policy_version: "v1".into(),
        },
    }
}

#[test]
fn replay_capsule_validate_accepts_well_formed_capsule() {
    let capsule = base_capsule();
    assert!(capsule.validate().is_ok());
}

#[test]
fn replay_capsule_validate_rejects_empty_events() {
    let mut capsule = base_capsule();
    capsule.event_ids.clear();
    assert!(capsule.validate().is_err());
}

#[test]
fn replay_capsule_validate_rejects_empty_graph() {
    let mut capsule = base_capsule();
    capsule.state_graph.nodes.clear();
    assert!(capsule.validate().is_err());
}
