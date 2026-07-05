use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;
use std::fs;
use std::process::Command;

fn unique_db(label: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dak_{}_{}.db", label, nanos))
        .display()
        .to_string()
}

#[test]
fn semantic_artifacts_cli_lists_rows_for_task_and_step() {
    let db_s = unique_db("semantic_artifacts_list");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bus = EventBus::new(db).unwrap();
    bus.append_semantic_artifact(
        "task-list",
        "analyze",
        1,
        "analysis_seed",
        &json!({"seed":"one"}),
    )
    .unwrap();
    bus.append_semantic_artifact(
        "task-list",
        "analyze",
        2,
        "analysis_seed",
        &json!({"seed":"two"}),
    )
    .unwrap();
    bus.append_semantic_artifact(
        "task-list",
        "plan",
        3,
        "classification",
        &json!({"class":"p"}),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(["semantic-artifacts", "task-list", "analyze"])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\"seed\":\"one\""));
    assert!(stdout.contains("\"seed\":\"two\""));
    assert!(!stdout.contains("\"plan\":\"p\""));

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
