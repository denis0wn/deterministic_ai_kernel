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

fn classify(
    a: &deterministic_ai_kernel::kernel_types::ReplayCapsule,
    b: &deterministic_ai_kernel::kernel_types::ReplayCapsule,
) -> &'static str {
    if a.validate().is_err() || b.validate().is_err() {
        "structurally_invalid"
    } else if a.event_ids == b.event_ids
        && a.state_graph.nodes == b.state_graph.nodes
        && a.state_graph.edges == b.state_graph.edges
    {
        "identical"
    } else {
        "divergent"
    }
}

#[test]
fn replay_capsule_classification_returns_expected_labels() {
    let db = unique_db_path("replay_capsule_classification");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();

    bus.append_event(
        "task-a",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();
    bus.append_event(
        "task-a",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .unwrap();

    bus.append_event(
        "task-b",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();

    let a = build_replay_capsule(&bus, "task-a").unwrap();
    let b = build_replay_capsule(&bus, "task-b").unwrap();

    assert_eq!(classify(&a, &a), "identical");
    assert_eq!(classify(&a, &b), "divergent");

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
