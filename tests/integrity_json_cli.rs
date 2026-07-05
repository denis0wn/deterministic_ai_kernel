mod cli_json_contract;

use cli_json_contract::assert_cli_json_v1;
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
fn integrity_json_cli_emits_valid_json_report() {
    let db = unique_db_path("integrity_json_cli");
    let _ = fs::remove_file(&db);

    let (out, success) = run_kernel(&db, &["integrity-json"]);
    assert!(success, "integrity-json failed: {}", out);

    let parsed: Value = serde_json::from_str(&out).expect(&out);
    assert_cli_json_v1(&parsed, "integrity-json");
    assert_eq!(parsed["report"]["snapshot_version"], 1);
    assert_eq!(parsed["report"]["schema_version"], 1);
    assert_eq!(parsed["report"]["created_at_present"], true);
    assert_eq!(parsed["report"]["state_hash_present"], true);
    assert_eq!(parsed["report"]["state_present"], true);
    assert_eq!(parsed["report"]["task_id"], "integrity-task");

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
