//! Phase 4 §3 — Artifact Immutability Contract

use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;

fn unique_db(name: &str) -> String {
    format!("/tmp/artifact_immut_{}.db", name)
}

#[test]
fn artifact_source_generation_is_monotonically_increasing() {
    let db = unique_db("monotonic");
    let bus = EventBus::new(&db).unwrap();

    for i in 0..3i64 {
        bus.append_semantic_artifact("task-x", "00_analyze", i, "semantic_bias_v1",
            &json!({"preferred_field": format!("h{i}")})).unwrap();
    }

    let artifacts = bus.list_semantic_artifacts("task-x", Some("00_analyze")).unwrap();
    // list возвращает DESC — разворачиваем
    let mut gens: Vec<i64> = artifacts.iter().map(|a| a.source_generation).collect();
    gens.reverse();
    for w in gens.windows(2) {
        assert!(w[0] < w[1], "source_generation not monotonic: {:?}", gens);
    }
    let _ = std::fs::remove_file(&db);
}

#[test]
fn artifact_older_version_content_is_preserved() {
    let db = unique_db("preserved");
    let bus = EventBus::new(&db).unwrap();

    bus.append_semantic_artifact("task-y", "01_plan", 1, "semantic_bias_v1",
        &json!({"preferred_field": "h1"})).unwrap();
    bus.append_semantic_artifact("task-y", "01_plan", 2, "semantic_bias_v1",
        &json!({"preferred_field": "h2"})).unwrap();

    let artifacts = bus.list_semantic_artifacts("task-y", Some("01_plan")).unwrap();
    // DESC порядок — последний элемент = самый старый (gen=1)
    let oldest = artifacts.last().expect("must have rows");
    assert!(oldest.payload.contains("h1"),
        "oldest payload must be preserved, got: {}", oldest.payload);
    let _ = std::fs::remove_file(&db);
}

#[test]
fn artifact_count_matches_writes() {
    let db = unique_db("count");
    let bus = EventBus::new(&db).unwrap();

    for i in 0..5i64 {
        bus.append_semantic_artifact("task-z", "02_execute", i, "semantic_bias_v1",
            &json!({"preferred_field": format!("h{i}")})).unwrap();
    }

    let artifacts = bus.list_semantic_artifacts("task-z", Some("02_execute")).unwrap();
    assert_eq!(artifacts.len(), 5, "expected 5 artifacts, got {}", artifacts.len());
    let _ = std::fs::remove_file(&db);
}
