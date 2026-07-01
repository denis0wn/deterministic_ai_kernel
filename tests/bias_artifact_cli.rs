use serde_json::Value;
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

fn run_kernel(db: &Path, args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .env("KERNEL_DB_PATH", db.as_os_str())
        .args(args)
        .output()
        .expect("failed to run kernel")
}

#[test]
fn emit_bias_artifact_persists_semantic_bias_v1() {
    let db = unique_db_path("bias_artifact_cli");
    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));

    let emit = run_kernel(
        &db,
        &[
            "emit-bias-artifact",
            "task-bias",
            "step-bias",
            "AnalyzeTask",
            "ExecuteChanges",
        ],
    );

    assert!(
        emit.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&emit.stdout),
        String::from_utf8_lossy(&emit.stderr)
    );

    let list = run_kernel(&db, &["latest-bias-artifact", "task-bias", "step-bias"]);
    assert!(list.status.success());

    let stdout = String::from_utf8(list.stdout).unwrap();
    let line = stdout.lines().next().expect("expected one semantic artifact row");
    let cols: Vec<&str> = line.splitn(6, '\t').collect();

    assert_eq!(cols.len(), 6);
    assert_eq!(cols[1], "task-bias");
    assert_eq!(cols[2], "step-bias");
    assert_eq!(cols[4], "semantic_bias_v1");

    let payload: Value = serde_json::from_str(cols[5]).unwrap();

    assert_eq!(payload["version"], 1);
    assert_eq!(payload["seed"], 0);
    assert_eq!(payload["preferred"], serde_json::json!(["AnalyzeTask", "ExecuteChanges"]));

    let lines = payload["lines"].as_array().expect("lines must be an array");
    let rendered: Vec<&str> = lines.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(rendered.contains(&"bias.version=1"));
    assert!(rendered.contains(&"bias.meta.weighted_count=2"));
    assert!(rendered.contains(&"bias.weight.AnalyzeTask=1.000000"));
    assert!(rendered.contains(&"bias.weight.ExecuteChanges=1.000000"));

    let weights = payload["weights"].as_object().expect("weights must be an object");
    assert_eq!(weights["AnalyzeTask"], 1.0);
    assert_eq!(weights["ExecuteChanges"], 1.0);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
