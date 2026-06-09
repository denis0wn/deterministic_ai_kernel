use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;
use std::fs;

#[test]
fn latest_analysis_seed_returns_most_recent_artifact() {
    let db = "semantic_artifacts_test.db";
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bus = EventBus::new(db).unwrap();

    bus.append_semantic_artifact(
        "task-1",
        "analyze",
        1,
        "analysis_seed",
        &json!({"seed": "old"}),
    )
    .unwrap();

    bus.append_semantic_artifact(
        "task-1",
        "analyze",
        2,
        "analysis_seed",
        &json!({"seed": "new"}),
    )
    .unwrap();

    let latest = bus.latest_analysis_seed("task-1", Some("analyze")).unwrap().unwrap();
    assert_eq!(latest.source_generation, 2);
    assert_eq!(latest.artifact_type, "analysis_seed");
    assert!(latest.payload.contains("\"seed\":\"new\""));

    let rows = bus.list_semantic_artifacts("task-1", Some("analyze")).unwrap();
    assert_eq!(rows.len(), 2);

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
