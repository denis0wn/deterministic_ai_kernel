//! Phase 4 §3 — Artifact Immutability Contract

use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;

fn unique_db(label: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    let db_path = std::env::temp_dir()
        .join(format!("dak_art_immut_{}_{}.db", label, nanos))
        .display()
        .to_string();
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path));
    let _ = std::fs::remove_file(format!("{}-shm", db_path));
    db_path
}

#[test]
fn artifact_source_generation_is_monotonically_increasing() {
    let db = unique_db("monotonic");
    let bus = EventBus::new(&db).expect("test failure");

    for i in 0..3i64 {
        bus.append_semantic_artifact(
            "task-x",
            "00_analyze",
            i,
            "semantic_bias_v1",
            &json!({"preferred_field": format!("h{i}")}),
        )
        .expect("test failure");
    }

    let artifacts = bus
        .list_semantic_artifacts("task-x", Some("00_analyze"))
        .expect("test failure");
    // list возвращает DESC — разворачиваем
    let mut gens: Vec<i64> = artifacts.iter().map(|a| a.source_generation).collect();
    gens.reverse();
    for w in gens.windows(2) {
        assert!(w[0] < w[1], "source_generation not monotonic: {:?}", gens);
    }
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(format!("{}-wal", db));
    let _ = std::fs::remove_file(format!("{}-shm", db));
}

#[test]
fn artifact_older_version_content_is_preserved() {
    let db = unique_db("preserved");
    let bus = EventBus::new(&db).expect("test failure");

    bus.append_semantic_artifact(
        "task-y",
        "01_plan",
        1,
        "semantic_bias_v1",
        &json!({"preferred_field": "h1"}),
    )
    .expect("test failure");
    bus.append_semantic_artifact(
        "task-y",
        "01_plan",
        2,
        "semantic_bias_v1",
        &json!({"preferred_field": "h2"}),
    )
    .expect("test failure");

    let artifacts = bus
        .list_semantic_artifacts("task-y", Some("01_plan"))
        .expect("test failure");
    // DESC порядок — последний элемент = самый старый (gen=1)
    let oldest = artifacts.last().expect("must have rows");
    assert!(
        oldest.payload.contains("h1"),
        "oldest payload must be preserved, got: {}",
        oldest.payload
    );
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(format!("{}-wal", db));
    let _ = std::fs::remove_file(format!("{}-shm", db));
}

#[test]
fn artifact_count_matches_writes() {
    let db = unique_db("count");
    let bus = EventBus::new(&db).expect("test failure");

    for i in 0..5i64 {
        bus.append_semantic_artifact(
            "task-z",
            "02_execute",
            i,
            "semantic_bias_v1",
            &json!({"preferred_field": format!("h{i}")}),
        )
        .expect("test failure");
    }

    let artifacts = bus
        .list_semantic_artifacts("task-z", Some("02_execute"))
        .expect("test failure");
    assert_eq!(
        artifacts.len(),
        5,
        "expected 5 artifacts, got {}",
        artifacts.len()
    );
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(format!("{}-wal", db));
    let _ = std::fs::remove_file(format!("{}-shm", db));
}
