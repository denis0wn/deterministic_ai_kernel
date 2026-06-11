use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("deterministic_ai_kernel_{}_{}.db", test_name, nanos))
}

#[test]
fn replay_capsule_diff_detects_identical_capsules() {
    let db = unique_db_path("replay_capsule_diff");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();

    bus.append_event(
        "task-diff",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    ).unwrap();
    bus.append_event(
        "task-diff",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    ).unwrap();

    let left = build_replay_capsule(&bus, "task-diff").unwrap();
    let right = build_replay_capsule(&bus, "task-diff").unwrap();

    assert_eq!(left.event_ids, right.event_ids);
    assert_eq!(left.state_graph.nodes, right.state_graph.nodes);
    assert_eq!(left.state_graph.edges, right.state_graph.edges);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
