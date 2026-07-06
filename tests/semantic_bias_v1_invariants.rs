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
fn semantic_bias_v1_contract_remains_sealed() {
    let db_s = unique_db("semantic_bias_v1_invariants");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bin = env!("CARGO_BIN_EXE_deterministic_ai_kernel");

    let emit = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args([
            "emit-bias-artifact",
            "task-sealed",
            "step-sealed",
            "AnalyzeTask",
            "ExecuteChanges",
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
        .args(["latest-bias-artifact", "task-sealed", "step-sealed"])
        .output()
        .unwrap();

    assert!(
        latest.status.success(),
        "latest stderr=\n{}",
        String::from_utf8_lossy(&latest.stderr)
    );

    let latest_stdout = String::from_utf8(latest.stdout).unwrap();
    let line = latest_stdout.lines().next().expect("expected a row");
    let cols: Vec<&str> = line.splitn(6, '\t').collect();
    let payload: Value = serde_json::from_str(cols[5]).unwrap();

    let expected_keys = ["version", "seed", "preferred", "weights", "lines"];
    let mut keys: Vec<&str> = payload
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();
    let mut expected = expected_keys.to_vec();
    keys.sort_unstable();
    expected.sort_unstable();
    assert_eq!(keys, expected);

    let snapshot = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["snapshot", "task-sealed"])
        .output()
        .unwrap();

    assert!(
        snapshot.status.success(),
        "snapshot stderr=\n{}",
        String::from_utf8_lossy(&snapshot.stderr)
    );

    let restore = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["restore", "task-sealed"])
        .output()
        .unwrap();

    assert!(
        restore.status.success(),
        "restore stderr=\n{}",
        String::from_utf8_lossy(&restore.stderr)
    );

    let restore_stdout = String::from_utf8(restore.stdout).unwrap();
    let json_line = restore_stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("expected JSON payload");

    let snapshot_payload: Value = serde_json::from_str(json_line).unwrap();

    let mut artifact_keys: Vec<&str> = snapshot_payload["artifacts"]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| k.as_str())
        .collect();

    artifact_keys.sort_unstable();
    assert_eq!(artifact_keys, ["semantic_bias_v1"]);

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
