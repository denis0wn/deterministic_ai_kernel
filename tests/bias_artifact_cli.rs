use serde_json::Value;
use std::fs;
use std::process::Command;

#[test]
fn emit_bias_artifact_persists_semantic_bias_v1() {
    let db = "bias_artifact_cli_test.db";
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let emit = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args([
            "emit-bias-artifact",
            "task-bias",
            "step-bias",
            "AnalyzeTask",
            "ExecuteChanges",
        ])
        .output()
        .unwrap();

    assert!(
        emit.status.success(),
        "stdout=\n{}\n\nstderr=\n{}",
        String::from_utf8_lossy(&emit.stdout),
        String::from_utf8_lossy(&emit.stderr)
    );

    let list = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(["latest-bias-artifact", "task-bias", "step-bias"])
        .output()
        .unwrap();

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

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
