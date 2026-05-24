use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("deterministic_ai_kernel_{}_{}.db", test_name, nanos))
}

fn run(db: &PathBuf, args: &[&str]) -> String {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .env("KERNEL_DB_PATH", db.as_os_str())
        .args(args)
        .output()
        .expect("failed to run command");
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn run_expect_fail(db: &PathBuf, args: &[&str]) -> String {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--"])
        .env("KERNEL_DB_PATH", db.as_os_str())
        .args(args)
        .output()
        .expect("failed to run command");
    assert!(
        !out.status.success(),
        "expected failure, got success:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn sqlite(db: &PathBuf, sql: &str) {
    let out = Command::new("sqlite3")
        .arg(db)
        .arg(sql)
        .output()
        .expect("failed to run sqlite3");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn setup_task(db: &PathBuf, task_id: &str) {
    let _ = fs::remove_file(db);

    let schema_and_seed = format!(
        r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS event_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  system_generation INTEGER NOT NULL,
  causal_unit_id INTEGER NOT NULL,
  sequence_in_unit INTEGER NOT NULL,
  task_id TEXT NOT NULL,
  step_id TEXT,
  event_type TEXT NOT NULL,
  payload TEXT NOT NULL,
  logical_generation INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS step_dependencies (
  task_id TEXT NOT NULL,
  step_id TEXT NOT NULL,
  depends_on_step_id TEXT NOT NULL,
  UNIQUE(task_id, step_id, depends_on_step_id)
);

CREATE TABLE IF NOT EXISTS step_status (
  task_id TEXT NOT NULL,
  step_id TEXT NOT NULL,
  status TEXT NOT NULL,
  UNIQUE(task_id, step_id)
);

CREATE TABLE IF NOT EXISTS leases (
  lease_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  step_id TEXT NOT NULL,
  worker_id TEXT NOT NULL,
  acquired_generation INTEGER NOT NULL,
  expires_at_generation INTEGER NOT NULL,
  state TEXT NOT NULL
);

INSERT OR IGNORE INTO step_status (task_id, step_id, status) VALUES
('{0}','step_0','pending'),
('{0}','step_1','pending'),
('{0}','step_2','pending');

INSERT OR IGNORE INTO step_dependencies (task_id, step_id, depends_on_step_id) VALUES
('{0}','step_1','step_0'),
('{0}','step_2','step_1');
"#,
        task_id
    );

    sqlite(db, &schema_and_seed);
}

#[test]
fn happy_path_chain_completes() {
    let db = unique_db_path("happy_path_chain_completes");
    setup_task(&db, "task_test");

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(&db, &["start-step", "task_test", "worker-A", "step_0"]);
    run(&db, &["complete-step", "task_test", "worker-A", "step_0"]);

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(&db, &["start-step", "task_test", "worker-A", "step_1"]);
    run(&db, &["complete-step", "task_test", "worker-A", "step_1"]);

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(&db, &["start-step", "task_test", "worker-A", "step_2"]);
    run(&db, &["complete-step", "task_test", "worker-A", "step_2"]);

    let out = Command::new("sqlite3")
        .arg(&db)
        .arg("select step_id || '|' || status from step_status where task_id='task_test' order by step_id;")
        .output()
        .expect("failed to inspect db");

    let statuses = String::from_utf8_lossy(&out.stdout);
    assert!(statuses.contains("step_0|committed"));
    assert!(statuses.contains("step_1|committed"));
    assert!(statuses.contains("step_2|committed"));

    let _ = fs::remove_file(&db);
}

#[test]
fn stale_worker_is_rejected_after_reclaim() {
    let db = unique_db_path("stale_worker_is_rejected_after_reclaim");
    setup_task(&db, "task_test2");

    run(&db, &["schedule", "task_test2"]);
    run(&db, &["claim-worker", "task_test2", "worker-A"]);
    run(&db, &["start-step", "task_test2", "worker-A", "step_0"]);
    run(&db, &["expire-leases", "task_test2"]);
    run(&db, &["reconcile", "task_test2"]);
    run(&db, &["schedule", "task_test2"]);
    run(&db, &["claim-worker", "task_test2", "worker-B"]);

    let err = run_expect_fail(&db, &["complete-step", "task_test2", "worker-A", "step_0"]);
    assert!(err.contains("no active lease owned by worker for step"));

    let _ = fs::remove_file(&db);
}
