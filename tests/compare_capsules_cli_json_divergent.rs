mod cli_json_contract;

use cli_json_contract::assert_cli_json_v1;
use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use serde_json::{json, Value};
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

    let raw = String::from_utf8_lossy(&out.stdout);
    let text = if let Some(pos) = raw.find(['{', '[']) {
        raw[pos..].trim_end().to_string()
    } else {
        raw.trim().to_string()
    };
    (text, out.status.success())
}

#[test]
fn compare_capsules_json_reports_divergent_with_diff_payload() {
    let db = unique_db_path("compare_capsules_cli_json_divergent");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();

    bus.append_event(
        "task-left-json",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();
    bus.append_event(
        "task-left-json",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .unwrap();

    bus.append_event(
        "task-right-json",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();

    let left_capsule = build_replay_capsule(&bus, "task-left-json").unwrap();
    let right_capsule = build_replay_capsule(&bus, "task-right-json").unwrap();
    bus.save_replay_capsule(&left_capsule).unwrap();
    bus.save_replay_capsule(&right_capsule).unwrap();
    drop(bus);

    let (out, success) = run_kernel(
        &db,
        &[
            "compare-capsules",
            "task-left-json",
            "task-right-json",
            "--json",
        ],
    );
    assert!(success, "compare-capsules failed: {}", out);

    let parsed: Value = serde_json::from_str(&out).expect(&out);
    assert_cli_json_v1(&parsed, "compare-capsules");
    assert_eq!(parsed["report"]["status"], "divergent");
    assert!(parsed["report"]["explanation"]
        .as_str()
        .unwrap()
        .contains("event_ids differ"));
    assert!(!parsed["report"]["diff"]["event_ids"]["left_only"]
        .as_array()
        .unwrap()
        .is_empty());

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
