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
    std::env::temp_dir().join(format!(
        "deterministic_ai_kernel_{}_{}.db",
        test_name, nanos
    ))
}

#[test]
fn replay_capsule_is_built_from_task_event_stream() {
    let db = unique_db_path("replay_capsule_builder");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();

    bus.append_event(
        "task-capsule",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();

    bus.append_event(
        "task-capsule",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .unwrap();

    let capsule = build_replay_capsule(&bus, "task-capsule").unwrap();

    assert_eq!(capsule.execution_id, "task-capsule");
    assert_eq!(capsule.event_ids.len(), 2);
    assert_eq!(capsule.state_graph.nodes.len(), 2);
    assert_eq!(capsule.state_graph.edges.len(), 1);
    assert!(capsule.is_minimally_valid());

    let raw = serde_json::to_string_pretty(&capsule).unwrap();
    assert!(raw.contains("task-capsule"));
    assert!(raw.contains("determinism_envelope"));

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
