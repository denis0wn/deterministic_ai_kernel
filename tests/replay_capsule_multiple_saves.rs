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
fn latest_replay_capsule_returns_most_recent_capsule_for_same_task() {
    let db = unique_db_path("replay_capsule_multiple_saves");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).expect("test failure");

    bus.append_event(
        "task-multi-save",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .expect("test failure");

    let first = build_replay_capsule(&bus, "task-multi-save").expect("test failure");
    bus.save_replay_capsule(&first).expect("test failure");

    bus.append_event(
        "task-multi-save",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .expect("test failure");

    let second = build_replay_capsule(&bus, "task-multi-save").expect("test failure");
    bus.save_replay_capsule(&second).expect("test failure");

    let loaded = bus
        .latest_replay_capsule("task-multi-save")
        .expect("test failure")
        .expect("capsule missing");

    assert_eq!(loaded.execution_id, "task-multi-save");
    assert_eq!(loaded.event_ids.len(), 2);
    assert_eq!(loaded.state_graph.nodes.len(), 2);
    assert_eq!(loaded.state_graph.edges.len(), 1);
    assert_eq!(loaded.capsule_id, second.capsule_id);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
