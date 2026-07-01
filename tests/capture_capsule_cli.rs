use deterministic_ai_kernel::event_bus::EventBus;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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

fn run_kernel(db: &Path, args: &[&str]) -> (String, bool) {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .env("KERNEL_DB_PATH", db.as_os_str())
        .args(args)
        .output()
        .expect("failed to run kernel");

    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (text, out.status.success())
}

#[test]
fn capture_capsule_cli_outputs_capsule_json() {
    let db = unique_db_path("capture_capsule_cli");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();
    bus.append_event(
        "task-cli-capsule",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();
    bus.append_event(
        "task-cli-capsule",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .unwrap();

    let (out, success) = run_kernel(&db, &["capture-capsule", "task-cli-capsule"]);
    assert!(success, "capture-capsule failed: {}", out);
    assert!(
        out.contains("\"execution_id\": \"task-cli-capsule\""),
        "{}",
        out
    );
    assert!(out.contains("\"event_ids\""), "{}", out);
    assert!(out.contains("\"determinism_envelope\""), "{}", out);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
