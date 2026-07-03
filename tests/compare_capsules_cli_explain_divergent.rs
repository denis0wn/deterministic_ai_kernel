use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
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
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db.as_os_str())
        .args(args)
        .output()
        .expect("failed to run kernel");

    let text =
        String::from_utf8_lossy(&out.stdout).to_string();
    (text, out.status.success())
}

#[test]
fn compare_capsules_explain_reports_divergent_reason() {
    let db = unique_db_path("compare_capsules_cli_explain_divergent");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();

    bus.append_event(
        "task-left",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();
    bus.append_event(
        "task-left",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .unwrap();

    bus.append_event(
        "task-right",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();

    let left_capsule = build_replay_capsule(&bus, "task-left").unwrap();
    let right_capsule = build_replay_capsule(&bus, "task-right").unwrap();
    bus.save_replay_capsule(&left_capsule).unwrap();
    bus.save_replay_capsule(&right_capsule).unwrap();

    let (out, success) = run_kernel(
        &db,
        &["compare-capsules", "task-left", "task-right", "--explain"],
    );
    assert!(success, "compare-capsules failed: {}", out);
    assert!(out.contains("status=divergent"), "{}", out);
    assert!(out.contains("explanation="), "{}", out);
    assert!(out.contains("event_ids differ"), "{}", out);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
