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
fn semantic_artifact_type_contract_is_explicit() {
    let db_s = unique_db("semantic_artifact_contract");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bus = EventBus::new(db).expect("test failure");

    for artifact_type in [
        "analysis_seed",
        "retrieval_result",
        "classification",
        "semantic_bias_v1",
    ] {
        bus.append_semantic_artifact("task-1", "step-1", 1, artifact_type, &json!({"ok": true}))
            .expect("test failure");
    }

    let err = bus
        .append_semantic_artifact(
            "task-1",
            "step-1",
            2,
            "execution_plan",
            &json!({"ok": false}),
        )
        .expect_err("expected test error");

    let msg = format!("{err:#}");
    assert!(msg.contains("CHECK constraint failed") || msg.contains("constraint failed"));

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
