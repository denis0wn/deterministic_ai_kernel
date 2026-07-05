use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "deterministic_ai_kernel_{}_{}.db",
        test_name, nanos
    ))
}

fn run_kernel(db: &Path, args: &[&str]) -> (String, bool) {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .env("KERNEL_DB_PATH", db.as_os_str())
        .args(args)
        .output()
        .expect("failed to run kernel");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    (text, out.status.success())
}

#[test]
fn replay_and_snapshot_do_not_panic_on_clean_db() {
    let db = unique_db_path("replay_snapshot");
    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));

    let (out, success) = run_kernel(&db, &["reset"]);
    assert!(success, "reset failed: {}", out);

    let (out, success) = run_kernel(&db, &["replay", "empty-task"]);
    assert!(success, "replay failed: {}", out);
    assert!(
        out.contains("REPLAY OK: true"),
        "replay should be valid: {}",
        out
    );

    let (out, _success) = run_kernel(&db, &["snapshot", "empty-task"]);
    assert!(!out.contains("panicked"), "snapshot panicked: {}", out);

    let (out, _success) = run_kernel(&db, &["restore", "empty-task"]);
    assert!(!out.contains("panicked"), "restore panicked: {}", out);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
