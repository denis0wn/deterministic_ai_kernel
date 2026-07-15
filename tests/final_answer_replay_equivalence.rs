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

fn load_final_answer(bus: &EventBus, task_id: &str) -> String {
    bus.list_semantic_artifacts(task_id, None)
        .expect("list semantic artifacts")
        .into_iter()
        .rev()
        .find(|a| a.artifact_type == "final_answer")
        .and_then(|a| serde_json::from_str::<serde_json::Value>(&a.payload).ok())
        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .expect("final_answer artifact")
}

#[test]
fn final_answer_reconstruction_is_equivalent() {
    let db_s = unique_db("final_answer_replay_equivalence");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bus = EventBus::new(db).expect("bus");
    let original = "reconstructed final answer must stay identical";

    bus.append_semantic_artifact(
        "task-replay-final-answer",
        "02_execute",
        9,
        "final_answer",
        &json!({ "text": original }),
    )
    .expect("append final answer");

    let before = load_final_answer(&bus, "task-replay-final-answer");

    let reconstructed_ok = deterministic_ai_kernel::reconstruction::reconstruct_state(
        db,
        "task-replay-final-answer",
    );
    assert!(reconstructed_ok);

    let bus_after = EventBus::new(db).expect("bus after");
    let after = load_final_answer(&bus_after, "task-replay-final-answer");

    assert_eq!(before, after);
    assert_eq!(after, original);
}
