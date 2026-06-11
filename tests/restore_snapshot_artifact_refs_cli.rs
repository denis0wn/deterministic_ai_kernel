use std::fs;
use std::process::Command;

#[test]
fn restore_snapshot_prints_artifact_ref_lines() {
    let db = "restore_snapshot_artifact_refs_cli_test.db";
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));

    let bin = env!("CARGO_BIN_EXE_deterministic_ai_kernel");

    let emit = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["emit-bias-artifact", "task-restore", "step-restore", "AnalyzeTask", "ExecuteChanges"])
        .output()
        .unwrap();
    assert!(emit.status.success(), "stderr=\n{}", String::from_utf8_lossy(&emit.stderr));

    let snapshot = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["snapshot", "task-restore"])
        .output()
        .unwrap();
    assert!(snapshot.status.success(), "stderr=\n{}", String::from_utf8_lossy(&snapshot.stderr));

    let restore = Command::new(bin)
        .env("KERNEL_DB_PATH", db)
        .args(["restore", "task-restore"])
        .output()
        .unwrap();
    assert!(restore.status.success(), "stderr=\n{}", String::from_utf8_lossy(&restore.stderr));

    let stdout = String::from_utf8(restore.stdout).unwrap();
    assert!(stdout.contains("ARTIFACT_REF\tsemantic_bias_v1\t"));
    assert!(stdout.contains("\"artifacts\":{\"semantic_bias_v1\":"));

    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}
