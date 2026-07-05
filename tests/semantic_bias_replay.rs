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
fn semantic_bias_v1_survives_replay_paths() {
    let db_s = unique_db("semantic_bias_replay");
    let db = db_s.as_str();
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let emit = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args([
            "emit-bias-artifact",
            "task-replay",
            "step-replay",
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
        ])
        .output()
        .unwrap();

    assert!(
        emit.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&emit.stderr)
    );

    let latest = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(["latest-bias-artifact", "task-replay", "step-replay"])
        .output()
        .unwrap();

    assert!(latest.status.success());

    let stdout = String::from_utf8(latest.stdout).unwrap();
    let line = stdout.lines().next().expect("expected a bias row");
    let cols: Vec<&str> = line.splitn(6, '\t').collect();

    assert_eq!(cols[1], "task-replay");
    assert_eq!(cols[2], "step-replay");
    assert_eq!(cols[4], "semantic_bias_v1");

    let payload: Value = serde_json::from_str(cols[5]).unwrap();
    assert_eq!(payload["version"], 1);
    assert_eq!(payload["seed"], 0);
    assert_eq!(
        payload["preferred"],
        serde_json::json!(["AnalyzeTask", "ExecuteChanges", "RunTests"])
    );

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
