use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir().join(format!("deterministic_ai_kernel_{}_{}.db", name, nanos))
}

fn cleanup(db: &PathBuf) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}

fn run_ok(db: &PathBuf, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .expect("test failure");

    assert!(
        out.status.success(),
        "command failed: {:?}\nstdout=\n{}\nstderr=\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    String::from_utf8(out.stdout).expect("test failure")
}

fn run_fail(db: &PathBuf, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .expect("test failure");

    assert!(
        !out.status.success(),
        "expected failure: {:?}\nstdout=\n{}\nstderr=\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn restore_payload(db: &PathBuf, task_id: &str) -> Value {
    let out = run_ok(db, &["restore", task_id]);
    assert!(out.contains("RESTORE OK"), "{out}");
    let json_line = out
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("expected restore payload json");
    serde_json::from_str(json_line).expect("test failure")
}

fn assert_snapshot_compatible(snapshot_version: &str, restore_version: &str) {
    let db = unique_db(&format!(
        "snapshot_matrix_{snapshot_version}_{restore_version}"
    ));
    cleanup(&db);

    let _ = run_ok(
        &db,
        &[
            "emit-bias-artifact",
            "matrix-task",
            "matrix-step",
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
        ],
    );

    let snapshot = run_ok(&db, &["snapshot", "matrix-task"]);
    assert!(snapshot.contains("SNAPSHOT OK"), "{snapshot}");

    let payload = restore_payload(&db, "matrix-task");
    assert_eq!(payload["snapshot_version"], 1);
    assert_eq!(payload["schema_version"], 1);
    assert!(payload["created_at"].is_number());
    assert!(payload["state_hash"].is_number());
    assert!(payload["state"].is_object());

    assert_eq!(payload["task_id"], "matrix-task");
    assert!(payload["artifacts"]["semantic_bias_v1"].is_number());

    cleanup(&db);
}

#[test]
fn snapshot_v1_to_restore_v1_is_compatible() {
    assert_snapshot_compatible("v1", "v1");
}

#[test]
fn snapshot_v1_to_restore_current_is_compatible() {
    assert_snapshot_compatible("v1", "current");
}

#[test]
fn snapshot_current_to_restore_current_is_compatible() {
    assert_snapshot_compatible("current", "current");
}

#[test]
fn snapshot_v2_policy_is_reserved_until_schema_exists() {
    let db = unique_db("snapshot_matrix_v2_policy_reserved");
    cleanup(&db);

    let _ = run_ok(
        &db,
        &[
            "emit-bias-artifact",
            "matrix-task-v2",
            "matrix-step",
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
        ],
    );

    let _ = run_ok(&db, &["snapshot", "matrix-task-v2"]);
    let payload = restore_payload(&db, "matrix-task-v2");
    assert_eq!(payload["snapshot_version"], 1, "{payload}");

    cleanup(&db);
}

#[test]
fn restore_future_version_rejected() {
    let db = unique_db("snapshot_matrix_future_version");
    cleanup(&db);

    let _ = run_ok(
        &db,
        &[
            "emit-bias-artifact",
            "future-task",
            "future-step",
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
        ],
    );
    let _ = run_ok(&db, &["snapshot", "future-task"]);

    let payload = restore_payload(&db, "future-task");
    let mut mutated = payload.clone();
    mutated["snapshot_version"] = Value::from(999u64);

    let json = serde_json::to_string(&mutated).expect("test failure");
    let escaped = json.replace('\'', "''");

    let sql = format!(
        "UPDATE state_snapshots SET payload = '{}' WHERE task_id = 'future-task';",
        escaped
    );

    let out = Command::new("sqlite3")
        .arg(&db)
        .arg(&sql)
        .output()
        .expect("failed to mutate snapshot payload");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let err = run_fail(&db, &["restore", "future-task"]);
    assert!(err.contains("unsupported future snapshot_version"), "{err}");

    cleanup(&db);
}
