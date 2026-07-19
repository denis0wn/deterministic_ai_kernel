use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;
use std::fs;

fn unique_db(label: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dak_{}_{}.db", label, nanos))
        .display()
        .to_string()
}

#[test]
fn final_answer_reads_from_semantic_artifacts() {
    let db_s = unique_db("final_answer_cli");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bus = EventBus::new(db).expect("bus");
    bus.append_semantic_artifact(
        "task-final-answer",
        "02_execute",
        7,
        "final_answer",
        &json!({ "text": "event sourced final answer" }),
    )
    .expect("append final answer");

    let rows = bus
        .list_semantic_artifacts("task-final-answer", None)
        .expect("list semantic artifacts");

    let answer = rows
        .into_iter()
        .rev()
        .find(|a| a.artifact_type == "final_answer")
        .and_then(|a| serde_json::from_str::<serde_json::Value>(&a.payload).ok())
        .and_then(|v| {
            v.get("text")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        });

    assert_eq!(answer.as_deref(), Some("event sourced final answer"));
}
