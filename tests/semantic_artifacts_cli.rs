use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;
use std::fs;
use std::process::Command;

#[test]
fn latest_analysis_seed_cli_prints_latest_row() {
    let db = "semantic_artifacts_cli_test.db";
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bus = EventBus::new(db).unwrap();
    bus.append_semantic_artifact("task-cli", "analyze", 1, "analysis_seed", &json!({"seed":"old"})).unwrap();
    bus.append_semantic_artifact("task-cli", "analyze", 2, "analysis_seed", &json!({"seed":"new"})).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(["latest-analysis-seed", "task-cli", "analyze"])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("task-cli"));
    assert!(stdout.contains("analyze"));
    assert!(stdout.contains("2"));
    assert!(stdout.contains("analysis_seed"));
    assert!(stdout.contains("\"seed\":\"new\""));

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
