use std::fs;
use std::process::Command;

#[test]
fn snapshot_artifacts_cli_prints_latest_snapshot_artifact_refs() {
    let db = "snapshot_artifacts_cli_test.db";
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bin = env!("CARGO_BIN_EXE_deterministic_ai_kernel");

    let emit = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args([
            "emit-bias-artifact",
            "task-artifacts",
            "step-artifacts",
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

    let snapshot = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["snapshot", "task-artifacts"])
        .output()
        .unwrap();
    assert!(
        snapshot.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&snapshot.stderr)
    );

    let out = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["snapshot-artifacts", "task-artifacts"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("ARTIFACT_REF\tsemantic_bias_v1\t"));

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
