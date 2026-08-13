use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "deterministic_ai_kernel_{}_{}.db",
        test_name, nanos
    ))
}

#[test]
fn replay_capsule_is_built_from_task_event_stream() {
    let db = unique_db_path("replay_capsule_builder");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).expect("test failure");

    bus.append_event(
        "task-capsule",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .expect("test failure");

    bus.append_event(
        "task-capsule",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .expect("test failure");

    let capsule = build_replay_capsule(&bus, "task-capsule").expect("test failure");

    assert_eq!(capsule.execution_id, "task-capsule");
    assert_eq!(capsule.event_ids.len(), 2);
    assert_eq!(capsule.state_graph.nodes.len(), 2);
    assert_eq!(capsule.state_graph.edges.len(), 1);
    assert!(capsule.is_minimally_valid());

    let raw = serde_json::to_string_pretty(&capsule).expect("test failure");
    assert!(raw.contains("task-capsule"));
    assert!(raw.contains("determinism_envelope"));

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}

#[test]
fn replay_capsule_created_at_is_deterministic() {
    let db = unique_db_path("capsule_determinism");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).expect("test failure");

    bus.append_event(
        "task-det",
        Some("00_step"),
        "STEP_STARTED",
        &json!({"step":"s0"}),
    )
    .expect("test failure");

    bus.append_event(
        "task-det",
        Some("00_step"),
        "STEP_COMPLETED",
        &json!({"step":"s0","outcome":"success"}),
    )
    .expect("test failure");

    // Build two capsules from the same event log
    let cap1 = build_replay_capsule(&bus, "task-det").expect("test failure");
    let cap2 = build_replay_capsule(&bus, "task-det").expect("test failure");

    // created_at must be identical (not "now" or wall clock)
    assert_eq!(cap1.created_at, cap2.created_at);
    assert_ne!(cap1.created_at, "now");
    // Must be derived from event data (starts with "evt-")
    assert!(cap1.created_at.starts_with("evt-"));

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
