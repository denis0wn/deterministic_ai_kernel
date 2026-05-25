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
('{0}','00_analyze_task','pending'),
('{0}','01_plan_execution','pending'),
('{0}','02_execute_changes','pending');

INSERT OR IGNORE INTO step_dependencies (task_id, step_id, depends_on_step_id) VALUES
('{0}','01_plan_execution','00_analyze_task'),
('{0}','02_execute_changes','01_plan_execution');
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
    run(&db, &["start-step", "task_test", "worker-A", "00_analyze_task"]);
    run(&db, &["complete-step", "task_test", "worker-A", "00_analyze_task"]);

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(&db, &["start-step", "task_test", "worker-A", "01_plan_execution"]);
    run(&db, &["complete-step", "task_test", "worker-A", "01_plan_execution"]);

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(&db, &["start-step", "task_test", "worker-A", "02_execute_changes"]);
    run(&db, &["complete-step", "task_test", "worker-A", "02_execute_changes"]);

    let out = Command::new("sqlite3")
        .arg(&db)
        .arg("select step_id || '|' || status from step_status where task_id='task_test' order by step_id;")
        .output()
        .expect("failed to inspect db");

    let statuses = String::from_utf8_lossy(&out.stdout);
    assert!(statuses.contains("00_analyze_task|committed"));
    assert!(statuses.contains("01_plan_execution|committed"));
    assert!(statuses.contains("02_execute_changes|committed"));

    let _ = fs::remove_file(&db);
}

#[test]
fn stale_worker_is_rejected_after_reclaim() {
    let db = unique_db_path("stale_worker_is_rejected_after_reclaim");
    setup_task(&db, "task_test2");

    run(&db, &["schedule", "task_test2"]);
    run(&db, &["claim-worker", "task_test2", "worker-A"]);
    run(&db, &["start-step", "task_test2", "worker-A", "00_analyze_task"]);
    run(&db, &["expire-leases", "task_test2"]);
    run(&db, &["reconcile", "task_test2"]);
    run(&db, &["schedule", "task_test2"]);
    run(&db, &["claim-worker", "task_test2", "worker-B"]);

    let err = run_expect_fail(&db, &["complete-step", "task_test2", "worker-A", "00_analyze_task"]);
    assert!(err.contains("no active lease owned by worker for step"));

    let _ = fs::remove_file(&db);
}


#[test]
fn retryable_failure_returns_step_to_pending() {
    let db = unique_db_path("retryable_failure_returns_step_to_pending");
    setup_task(&db, "task_retry");

    run(&db, &["schedule", "task_retry"]);
    run(&db, &["claim-worker", "task_retry", "worker-A"]);
    run(&db, &["start-step", "task_retry", "worker-A", "00_analyze_task"]);
    run(&db, &["fail-step", "task_retry", "worker-A", "00_analyze_task", "retry: network blip"]);
    run(&db, &["reconcile", "task_retry"]);
    run(&db, &["schedule", "task_retry"]);

    let out = Command::new("sqlite3")
        .arg(&db)
        .arg("select status from step_status where task_id='task_retry' and step_id='00_analyze_task';")
        .output()
        .expect("failed to inspect db");

    let status = String::from_utf8_lossy(&out.stdout);
    assert!(
        status.contains("ready") || status.contains("dispatched"),
        "expected retryable failed step to become ready or dispatched again, got: {}",
        status
    );

    let _ = fs::remove_file(&db);
}


#[test]
fn blocked_failure_returns_step_to_pending() {
    let db = unique_db_path("blocked_failure_returns_step_to_pending");
    setup_task(&db, "task_blocked");

    run(&db, &["schedule", "task_blocked"]);
    run(&db, &["claim-worker", "task_blocked", "worker-A"]);
    run(&db, &["start-step", "task_blocked", "worker-A", "00_analyze_task"]);
    run(&db, &["fail-step", "task_blocked", "worker-A", "00_analyze_task", "blocked: waiting_on dependency"]);
    run(&db, &["reconcile", "task_blocked"]);
    run(&db, &["schedule", "task_blocked"]);

    let out = Command::new("sqlite3")
        .arg(&db)
        .arg("select status from step_status where task_id='task_blocked' and step_id='00_analyze_task';")
        .output()
        .expect("failed to inspect db");

    let status = String::from_utf8_lossy(&out.stdout);
    assert!(
        status.contains("ready") || status.contains("dispatched"),
        "expected blocked failed step to become ready or dispatched again, got: {}",
        status
    );

    let _ = fs::remove_file(&db);
}

#[test]
fn claim_step_filters_by_capability() {
    use deterministic_ai_kernel::workflow::contract::WorkerCapability;
    use deterministic_ai_kernel::scheduler::{claim_step, seed_dependencies, schedule};

    let db = tmp_db();
    let task_id = "task-claim-cap";

    seed_dependencies(&db, task_id).unwrap();
    schedule(&db, task_id).unwrap();

    // Executor не должен получить первый шаг (Planner)
    let result = claim_step(&db, task_id, "worker-exec", WorkerCapability::Executor).unwrap();
    assert!(result.is_none(), "executor should not claim a planner step");

    // Planner должен получить первый шаг
    let result = claim_step(&db, task_id, "worker-plan", WorkerCapability::Planner).unwrap();
    assert!(result.is_some(), "planner should claim first ready step");
}
