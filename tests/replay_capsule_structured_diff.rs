use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use serde_json::json;
use std::collections::BTreeSet;
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
fn replay_capsule_structured_diff_detects_event_and_graph_differences() {
    let db = unique_db_path("replay_capsule_structured_diff");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();

    bus.append_event(
        "task-a",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    ).unwrap();
    bus.append_event(
        "task-a",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    ).unwrap();

    bus.append_event(
        "task-b",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    ).unwrap();

    let a = build_replay_capsule(&bus, "task-a").unwrap();
    let b = build_replay_capsule(&bus, "task-b").unwrap();

    let a_events: BTreeSet<_> = a.event_ids.iter().cloned().collect();
    let b_events: BTreeSet<_> = b.event_ids.iter().cloned().collect();

    assert_ne!(a_events, b_events);
    assert_ne!(a.state_graph.nodes, b.state_graph.nodes);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
