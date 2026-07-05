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
fn semantic_bias_observability_contract_is_consistent() {
    let db_s = unique_db("semantic_bias_observability_contract");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bin = env!("CARGO_BIN_EXE_deterministic_ai_kernel");

    let emit = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args([
            "emit-bias-artifact",
            "task-observe",
            "step-observe",
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
        ])
        .output()
        .unwrap();
    assert!(
        emit.status.success(),
        "emit stderr=\n{}",
        String::from_utf8_lossy(&emit.stderr)
    );

    let latest = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["latest-bias-artifact", "task-observe", "step-observe"])
        .output()
        .unwrap();
    assert!(
        latest.status.success(),
        "latest stderr=\n{}",
        String::from_utf8_lossy(&latest.stderr)
    );

    let latest_stdout = String::from_utf8(latest.stdout).unwrap();
    let latest_line = latest_stdout
        .lines()
        .next()
        .expect("expected latest bias row");
    let latest_cols: Vec<&str> = latest_line.splitn(6, '\t').collect();
    assert_eq!(latest_cols[1], "task-observe");
    assert_eq!(latest_cols[2], "step-observe");
    assert_eq!(latest_cols[4], "semantic_bias_v1");

    let latest_payload: Value = serde_json::from_str(latest_cols[5]).unwrap();

    let snapshot = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["snapshot", "task-observe"])
        .output()
        .unwrap();
    assert!(
        snapshot.status.success(),
        "snapshot stderr=\n{}",
        String::from_utf8_lossy(&snapshot.stderr)
    );

    let snapshot_artifacts = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["snapshot-artifacts", "task-observe"])
        .output()
        .unwrap();
    assert!(
        snapshot_artifacts.status.success(),
        "snapshot-artifacts stderr=\n{}",
        String::from_utf8_lossy(&snapshot_artifacts.stderr)
    );

    let snapshot_artifacts_stdout = String::from_utf8(snapshot_artifacts.stdout).unwrap();
    assert!(snapshot_artifacts_stdout.contains("ARTIFACT_REF\tsemantic_bias_v1\t"));

    let restore = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["restore", "task-observe"])
        .output()
        .unwrap();
    assert!(
        restore.status.success(),
        "restore stderr=\n{}",
        String::from_utf8_lossy(&restore.stderr)
    );

    let restore_stdout = String::from_utf8(restore.stdout).unwrap();
    assert!(restore_stdout.contains("ARTIFACT_REF\tsemantic_bias_v1\t"));

    let json_line = restore_stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("expected JSON payload in restore output");

    let restore_payload: Value = serde_json::from_str(json_line).unwrap();

    assert_eq!(restore_payload["task_id"], "task-observe");
    assert!(restore_payload["artifacts"]["semantic_bias_v1"].is_number());

    assert_eq!(latest_payload["version"], 1);
    assert_eq!(latest_payload["seed"], 0);
    assert_eq!(
        latest_payload["preferred"],
        serde_json::json!(["AnalyzeTask", "ExecuteChanges", "RunTests"])
    );

    let lines = latest_payload["lines"]
        .as_array()
        .expect("lines must be an array");
    let rendered: Vec<&str> = lines.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(rendered.contains(&"bias.version=1"));
    assert!(rendered.contains(&"bias.meta.preferred_count=3"));
    assert!(rendered.contains(&"bias.weight.AnalyzeTask=1.000000"));
    assert!(rendered.contains(&"bias.weight.ExecuteChanges=1.000000"));
    assert!(rendered.contains(&"bias.weight.RunTests=1.000000"));

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
