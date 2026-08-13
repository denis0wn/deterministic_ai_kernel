//! Phase 2, Task 3 — Replay Capsule Completeness
//!
//! Verifies that replay capsules fully capture artifacts, references, and provenance.

use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("capsule_complete_{}_{}.db", test_name, nanos))
}

fn cleanup(db: &PathBuf) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}

// ── 1. Artifact Capture ─────────────────────────────────────────────────────

#[test]
fn capsule_captures_semantic_artifacts() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("artifacts");
        let bus = EventBus::new(&db).unwrap();

        // Create events
        bus.append_event("task-art", Some("00_step"), "STEP_STARTED", &json!({}))
            .unwrap();
        bus.append_event("task-art", Some("00_step"), "STEP_COMPLETED", &json!({}))
            .unwrap();

        // Add semantic artifacts (only allowed types per schema)
        bus.append_semantic_artifact(
            "task-art",
            "00_step",
            1,
            "analysis_seed",
            &json!({"input": "test"}),
        )
        .unwrap();
        bus.append_semantic_artifact(
            "task-art",
            "00_step",
            1,
            "pipeline_step",
            &json!({"status": "ok"}),
        )
        .unwrap();

        let capsule = build_replay_capsule(&bus, "task-art").unwrap();

        // Verify artifacts are captured
        assert_eq!(capsule.artifacts.len(), 2);
        assert!(capsule
            .artifacts
            .iter()
            .any(|a| a.contains("analysis_seed")));
        assert!(capsule
            .artifacts
            .iter()
            .any(|a| a.contains("pipeline_step")));

        cleanup(&db);
    });
}

// ── 2. State Graph Completeness ────────────────────────────────────────────

#[test]
fn capsule_state_graph_matches_events() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("state_graph");
        let bus = EventBus::new(&db).unwrap();

        // Create 3 events
        for i in 0..3 {
            bus.append_event(
                "task-graph",
                Some(&format!("0{}_step", i)),
                "STEP_COMPLETED",
                &json!({"i": i}),
            )
            .unwrap();
        }

        let capsule = build_replay_capsule(&bus, "task-graph").unwrap();

        // Verify state graph has nodes for all events
        assert_eq!(capsule.state_graph.nodes.len(), 3);
        assert_eq!(capsule.event_ids.len(), 3);

        // Verify edges exist (sequential)
        assert!(capsule.state_graph.edges.len() >= 2);

        cleanup(&db);
    });
}

// ── 3. Provenance Fields ───────────────────────────────────────────────────

#[test]
fn capsule_has_required_provenance_fields() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("provenance");
        let bus = EventBus::new(&db).unwrap();

        bus.append_event("task-prov", Some("00_step"), "STEP_COMPLETED", &json!({}))
            .unwrap();

        let capsule = build_replay_capsule(&bus, "task-prov").unwrap();

        // Verify provenance fields are populated
        assert!(!capsule.capsule_id.is_empty());
        assert!(!capsule.execution_id.is_empty());
        assert!(!capsule.created_at.is_empty());
        assert_eq!(capsule.execution_id, "task-prov");
        assert!(capsule.capsule_id.starts_with("capsule-"));
        assert!(capsule.created_at.starts_with("evt-"));

        // Verify trust context
        assert_eq!(capsule.trust_context.source, "replay_capsule_builder");
        assert_eq!(
            capsule.trust_context.verification_status,
            "derived_from_event_log"
        );

        // Verify determinism envelope
        let envelope = capsule.determinism_envelope.as_object().unwrap();
        assert!(envelope.contains_key("inside"));
        assert!(envelope.contains_key("outside"));

        cleanup(&db);
    });
}

// ── 4. Determinism ─────────────────────────────────────────────────────────

#[test]
fn capsule_is_deterministic_for_identical_events() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("determinism");
        let bus = EventBus::new(&db).unwrap();

        // Create identical events
        bus.append_event("task-det", Some("00_a"), "STEP_STARTED", &json!({}))
            .unwrap();
        bus.append_event("task-det", Some("00_a"), "STEP_COMPLETED", &json!({}))
            .unwrap();

        // Build capsule twice
        let cap1 = build_replay_capsule(&bus, "task-det").unwrap();
        let cap2 = build_replay_capsule(&bus, "task-det").unwrap();

        // All fields must be identical
        assert_eq!(cap1.capsule_id, cap2.capsule_id);
        assert_eq!(cap1.execution_id, cap2.execution_id);
        assert_eq!(cap1.created_at, cap2.created_at);
        assert_eq!(cap1.event_ids, cap2.event_ids);
        assert_eq!(cap1.artifacts, cap2.artifacts);
        assert_eq!(cap1.state_graph, cap2.state_graph);
        assert_eq!(cap1.environment, cap2.environment);
        assert_eq!(cap1.determinism_envelope, cap2.determinism_envelope);
        assert_eq!(cap1.trust_context, cap2.trust_context);

        cleanup(&db);
    });
}

// ── 5. Empty Task Handling ─────────────────────────────────────────────────

#[test]
fn capsule_for_empty_task_has_minimal_structure() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("empty");
        let bus = EventBus::new(&db).unwrap();

        // No events
        let capsule = build_replay_capsule(&bus, "task-empty").unwrap();

        // Should have minimal valid structure
        assert_eq!(capsule.execution_id, "task-empty");
        assert!(capsule.capsule_id.starts_with("capsule-"));
        assert!(capsule.event_ids.is_empty());
        assert!(capsule.artifacts.is_empty());
        assert!(capsule.state_graph.nodes.is_empty());

        cleanup(&db);
    });
}
