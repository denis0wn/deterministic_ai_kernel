use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;
use std::fs;

fn unique_db(label: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dak_{}_{}.db", label, nanos))
        .display()
        .to_string()
}

#[test]
fn latest_analysis_seed_returns_most_recent_artifact() {
    let db_s = unique_db("semantic_artifacts");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bus = EventBus::new(db).expect("test failure");

    bus.append_semantic_artifact(
        "task-1",
        "analyze",
        1,
        "analysis_seed",
        &json!({"seed": "old"}),
    )
    .expect("test failure");

    bus.append_semantic_artifact(
        "task-1",
        "analyze",
        2,
        "analysis_seed",
        &json!({"seed": "new"}),
    )
    .expect("test failure");

    let latest = bus
        .latest_analysis_seed("task-1", Some("analyze"))
        .expect("test failure")
        .expect("test failure");
    assert_eq!(latest.source_generation, 2);
    assert_eq!(latest.artifact_type, "analysis_seed");
    assert!(latest.payload.contains("\"seed\":\"new\""));

    let rows = bus
        .list_semantic_artifacts("task-1", Some("analyze"))
        .expect("test failure");
    assert_eq!(rows.len(), 2);

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
