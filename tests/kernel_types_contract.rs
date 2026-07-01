use deterministic_ai_kernel::kernel_types::*;
use serde_json::json;
use std::collections::BTreeMap;

fn trust() -> TrustContext {
    TrustContext {
        source: "local.test".into(),
        trust_level: TrustLevel::High,
        verification_status: "verified".into(),
        policy_version: "v1".into(),
    }
}

#[test]
fn replay_capsule_v0_is_serializable_and_minimally_valid() {
    let event = ExecutionEvent {
        id: "evt-1".into(),
        task_id: "task-1".into(),
        timestamp: "2026-06-10T20:00:00Z".into(),
        event_type: "STEP_STARTED".into(),
        payload: json!({"step":"analyze_task"}),
        caused_by: None,
        trust_context: trust(),
    };

    let graph = StateGraph {
        nodes: vec![StateGraphNode {
            id: "n1".into(),
            kind: "event".into(),
            ref_id: event.id.clone(),
        }],
        edges: vec![],
    };

    let decision = DecisionPoint {
        id: "dp-1".into(),
        timestamp: "2026-06-10T20:00:01Z".into(),
        actor: "kernel".into(),
        evidence: vec![event.id.clone()],
        proposed_action: json!({"action":"capture_capsule"}),
        selected_action: json!({"action":"capture_capsule"}),
        confidence: Some(1.0),
        trust_context: trust(),
    };

    let capsule = ReplayCapsule {
        capsule_id: "cap-1".into(),
        execution_id: "exec-1".into(),
        created_at: "2026-06-10T20:00:02Z".into(),
        state_graph: graph,
        event_ids: vec![event.id],
        artifacts: vec![],
        environment: BTreeMap::from([
            ("os".into(), "macos".into()),
            ("mode".into(), "test".into()),
        ]),
        decision_points: vec![decision],
        determinism_envelope: json!({
            "inside": ["events", "state_graph", "artifacts"],
            "outside": ["wall_clock", "external_api"]
        }),
        trust_context: trust(),
    };

    let raw = serde_json::to_string_pretty(&capsule).unwrap();
    let roundtrip: ReplayCapsule = serde_json::from_str(&raw).unwrap();

    assert!(roundtrip.is_minimally_valid());
    assert_eq!(roundtrip.capsule_id, "cap-1");
    assert_eq!(roundtrip.decision_points.len(), 1);
    assert_eq!(roundtrip.event_ids.len(), 1);
}
