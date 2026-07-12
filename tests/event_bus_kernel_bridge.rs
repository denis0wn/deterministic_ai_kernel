use deterministic_ai_kernel::event_bus::EventBus;
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
fn event_bus_exposes_execution_events_and_state_graph() {
    let db = unique_db_path("event_bus_kernel_bridge");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).expect("test failure");

    bus.append_event(
        "task-bridge",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .expect("test failure");

    bus.append_event(
        "task-bridge",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .expect("test failure");

    let events = bus
        .list_execution_events("task-bridge")
        .expect("test failure");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "STEP_STARTED");
    assert_eq!(events[1].event_type, "STEP_COMPLETED");

    let graph = bus.build_state_graph("task-bridge").expect("test failure");
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
