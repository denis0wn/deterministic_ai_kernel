use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

fn run_kernel(db: &Path, args: &[&str]) -> (String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db.as_os_str())
        .args(args)
        .output()
        .expect("failed to run kernel");

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    (text, out.status.success())
}

#[test]
fn capture_capsule_save_json_reports_saved_capsule() {
    let db = unique_db_path("capture_capsule_save_cli_json");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).expect("test failure");
    bus.append_event(
        "task-capture-json",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .expect("test failure");
    bus.append_event(
        "task-capture-json",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .expect("test failure");

    let (out, success) = run_kernel(
        &db,
        &["capture-capsule-save", "task-capture-json", "--json"],
    );
    assert!(success, "capture-capsule-save failed: {}", out);

    let parsed: Value = serde_json::from_str(&out).expect(&out);
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["schema_version"], "cli-json-v1");
    assert_eq!(parsed["command"], "capture-capsule-save");
    assert_eq!(parsed["report"]["task_id"], "task-capture-json");
    assert_eq!(parsed["report"]["valid"], true);
    assert_eq!(parsed["report"]["events"], 2);
    assert!(parsed["report"]["capsule_id"]
        .as_str()
        .expect("test failure")
        .starts_with("capsule-"));

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
