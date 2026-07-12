use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::kernel_types::{ReplayCapsule, StateGraph, TrustContext, TrustLevel};
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use serde_json::json;
use std::collections::BTreeMap;
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
fn compare_capsules_explain_reports_invalid_reason() {
    let db = unique_db_path("compare_capsules_cli_explain_invalid");
    let _ = fs::remove_file(&db);

    let bus = EventBus::new(&db).expect("test failure");

    bus.append_event(
        "task-valid",
        Some("01_analyze_task"),
        "STEP_STARTED",
        &json!({"step":"analyze_task"}),
    )
    .expect("test failure");
    bus.append_event(
        "task-valid",
        Some("01_analyze_task"),
        "STEP_COMPLETED",
        &json!({"step":"analyze_task","outcome":"success"}),
    )
    .expect("test failure");

    let valid_capsule = build_replay_capsule(&bus, "task-valid").expect("test failure");
    bus.save_replay_capsule(&valid_capsule)
        .expect("test failure");

    let invalid_capsule = ReplayCapsule {
        capsule_id: "capsule-task-invalid".into(),
        execution_id: "task-invalid".into(),
        created_at: "now".into(),
        state_graph: StateGraph {
            nodes: vec![],
            edges: vec![],
        },
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
    bus.save_replay_capsule(&invalid_capsule)
        .expect("test failure");

    let (out, success) = run_kernel(
        &db,
        &[
            "compare-capsules",
            "task-valid",
            "task-invalid",
            "--explain",
        ],
    );
    assert!(success, "compare-capsules failed: {}", out);
    assert!(out.contains("status=structurally_invalid"), "{}", out);
    assert!(out.contains("explanation="), "{}", out);
    assert!(out.contains("invalid"), "{}", out);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
