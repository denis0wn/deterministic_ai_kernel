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
    let text = if let Some(pos) = raw.find(|c| c == '{' || c == '[') {
        raw[pos..].trim_end().to_string()
    } else {
        raw.trim().to_string()
    };
    (text, out.status.success())
}

#[test]
fn compare_capsules_json_reports_identical() {
    let db = unique_db_path("compare_capsules_cli_json_identical");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();
    bus.append_event(
        "task-identical-json",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .unwrap();
    bus.append_event(
        "task-identical-json",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .unwrap();

    let capsule = build_replay_capsule(&bus, "task-identical-json").unwrap();
    bus.save_replay_capsule(&capsule).unwrap();

    let (out, success) = run_kernel(
        &db,
        &[
            "compare-capsules",
            "task-identical-json",
            "task-identical-json",
            "--json",
        ],
    );
    assert!(success, "compare-capsules failed: {}", out);

    let parsed: Value = serde_json::from_str(&out).expect(&out);
    assert_cli_json_v1(&parsed, "compare-capsules");
    assert_eq!(parsed["schema_version"], "cli-json-v1");
    assert_eq!(parsed["report"]["status"], "identical");
    assert_eq!(parsed["report"]["left"]["valid"], true);
    assert_eq!(parsed["report"]["right"]["valid"], true);
    assert_eq!(
        parsed["report"]["diff"]["event_ids"]["left_only"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        parsed["report"]["diff"]["event_ids"]["right_only"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
