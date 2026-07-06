use serde_json::Value;
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
fn snapshot_payload_includes_semantic_bias_artifact_ref() {
    let db_s = unique_db("snapshot_artifact_refs");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bin = env!("CARGO_BIN_EXE_deterministic_ai_kernel");

    let emit = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args([
            "emit-bias-artifact",
            "task-snap",
            "step-snap",
            "AnalyzeTask",
            "ExecuteChanges",
        ])
        .output()
        .unwrap();
    assert!(
        emit.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&emit.stderr)
    );

    let rebuild = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["snapshot", "task-snap"])
        .output()
        .unwrap();
    assert!(
        rebuild.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&rebuild.stderr)
    );

    let restore = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["restore", "task-snap"])
        .output()
        .unwrap();
    assert!(
        restore.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&restore.stderr)
    );

    let stdout = String::from_utf8(restore.stdout).unwrap();

    // snapshot outputs: RESTORE OK, SNAPSHOT_ID, SNAPSHOT_GENERATION, then the JSON payload
    let json_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("expected JSON payload in restore output");

    let payload: Value = serde_json::from_str(json_line).unwrap();

    assert_eq!(payload["task_id"], "task-snap");
    assert!(
        payload["artifacts"]["semantic_bias_v1"].is_number(),
        "expected artifacts.semantic_bias_v1 to be an artifact_id number, got: {}",
        payload["artifacts"]["semantic_bias_v1"]
    );

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
