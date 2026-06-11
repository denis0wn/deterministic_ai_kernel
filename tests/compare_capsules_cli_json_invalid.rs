mod cli_json_contract;

use cli_json_contract::assert_cli_json_v1;
use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::kernel_types::{ReplayCapsule, StateGraph, TrustContext, TrustLevel};
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("deterministic_ai_kernel_{}_{}.db", test_name, nanos))
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
fn compare_capsules_json_reports_structurally_invalid() {
    let db = unique_db_path("compare_capsules_cli_json_invalid");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).unwrap();

    bus.append_event(
        "task-valid-json",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    ).unwrap();
    bus.append_event(
        "task-valid-json",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    ).unwrap();

    let valid_capsule = build_replay_capsule(&bus, "task-valid-json").unwrap();
    bus.save_replay_capsule(&valid_capsule).unwrap();

    let invalid_capsule = ReplayCapsule {
        capsule_id: "capsule-task-invalid-json".into(),
        execution_id: "task-invalid-json".into(),
        created_at: "now".into(),
        state_graph: StateGraph { nodes: vec![], edges: vec![] },
        event_ids: vec![],
        artifacts: vec![],
        environment: BTreeMap::from([("source".into(), "event_bus".into())]),
        decision_points: vec![],
        determinism_envelope: json!({"inside":["event_log"],"outside":["wall_clock"]}),
        trust_context: TrustContext {
            source: "test".into(),
            trust_level: TrustLevel::High,
            verification_status: "test".into(),
            policy_version: "v1".into(),
        },
    };
    bus.save_replay_capsule(&invalid_capsule).unwrap();

    let (out, success) = run_kernel(&db, &["compare-capsules", "task-valid-json", "task-invalid-json", "--json"]);
    assert!(success, "compare-capsules failed: {}", out);

    let parsed: Value = serde_json::from_str(&out).expect(&out);
    assert_cli_json_v1(&parsed, "compare-capsules");
    assert_eq!(parsed["report"]["status"], "structurally_invalid");
    assert_eq!(parsed["report"]["left"]["valid"], true);
    assert_eq!(parsed["report"]["right"]["valid"], false);
    assert!(parsed["report"]["explanation"].as_str().unwrap().contains("invalid"));

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
