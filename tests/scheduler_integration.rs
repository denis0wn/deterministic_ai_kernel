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

fn run(db: &Path, args: &[&str]) -> String {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--bin", "deterministic_ai_kernel", "--"])
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

fn run_expect_fail(db: &Path, args: &[&str]) -> String {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--bin", "deterministic_ai_kernel", "--"])
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

fn sqlite(db: &Path, sql: &str) {
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

fn setup_task(db: &Path, task_id: &str) {
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

CREATE TABLE IF NOT EXISTS tasks (
  task_id TEXT PRIMARY KEY,
  task_class TEXT NOT NULL
);

INSERT OR IGNORE INTO tasks (task_id, task_class) VALUES
('{0}','Generic');

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

fn setup_codefix_task(db: &Path, task_id: &str) {
    setup_task(db, task_id);
    sqlite(db, &format!(
        "UPDATE tasks SET task_class = 'CodeFix' WHERE task_id = '{0}';         DELETE FROM step_status WHERE task_id = '{0}';         DELETE FROM step_dependencies WHERE task_id = '{0}';         INSERT OR IGNORE INTO step_status (task_id, step_id, status) VALUES          ('{0}','00_read_repository','ready'),         ('{0}','01_locate_bug','pending'),         ('{0}','02_patch_code','pending'),         ('{0}','03_run_tests','pending'),         ('{0}','04_validate_patch','pending');         INSERT OR IGNORE INTO step_dependencies (task_id, step_id, depends_on_step_id) VALUES          ('{0}','01_locate_bug','00_read_repository'),         ('{0}','02_patch_code','01_locate_bug'),         ('{0}','03_run_tests','02_patch_code'),         ('{0}','04_validate_patch','03_run_tests');",
        task_id
    ));
}

#[test]
fn retry_and_reclaim_preserve_ownership_invariants() {
    let db = unique_db_path("retry_and_reclaim_preserve_ownership_invariants");
    setup_task(&db, "task_invariants");

    run(&db, &["schedule", "task_invariants"]);
    run(&db, &["claim-worker", "task_invariants", "worker-A"]);
    run(
        &db,
        &[
            "start-step",
            "task_invariants",
            "worker-A",
            "00_analyze_task",
        ],
    );

    run(&db, &["expire-leases", "task_invariants"]);
    run(&db, &["reconcile", "task_invariants"]);
    run(&db, &["schedule", "task_invariants"]);
    run(&db, &["claim-worker", "task_invariants", "worker-B"]);

    let err = run_expect_fail(
        &db,
        &[
            "complete-step",
            "task_invariants",
            "worker-A",
            "00_analyze_task",
        ],
    );
    assert!(
        err.contains("no active lease owned by worker for step"),
        "{err}"
    );

    let out = run(
        &db,
        &[
            "complete-step",
            "task_invariants",
            "worker-B",
            "00_analyze_task",
        ],
    );
    assert!(out.contains("COMPLETE"), "{out}");

    run(&db, &["schedule", "task_invariants"]);
    run(&db, &["claim-worker", "task_invariants", "worker-B"]);
    let out = run(
        &db,
        &[
            "start-step",
            "task_invariants",
            "worker-B",
            "01_plan_execution",
        ],
    );
    assert!(out.contains("STEP_RUNNING_OK"), "{out}");
}

#[test]
fn happy_path_chain_completes() {
    let db = unique_db_path("happy_path_chain_completes");
    setup_task(&db, "task_test");

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(
        &db,
        &["start-step", "task_test", "worker-A", "00_analyze_task"],
    );
    run(
        &db,
        &["complete-step", "task_test", "worker-A", "00_analyze_task"],
    );

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(
        &db,
        &["start-step", "task_test", "worker-A", "01_plan_execution"],
    );
    run(
        &db,
        &[
            "complete-step",
            "task_test",
            "worker-A",
            "01_plan_execution",
        ],
    );

    run(&db, &["schedule", "task_test"]);
    run(&db, &["claim-worker", "task_test", "worker-A"]);
    run(
        &db,
        &["start-step", "task_test", "worker-A", "02_execute_changes"],
    );
    run(
        &db,
        &[
            "complete-step",
            "task_test",
            "worker-A",
            "02_execute_changes",
        ],
    );

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
    run(
        &db,
        &["start-step", "task_test2", "worker-A", "00_analyze_task"],
    );
    run(&db, &["expire-leases", "task_test2"]);
    run(&db, &["reconcile", "task_test2"]);
    run(&db, &["schedule", "task_test2"]);
    run(&db, &["claim-worker", "task_test2", "worker-B"]);

    let err = run_expect_fail(
        &db,
        &["complete-step", "task_test2", "worker-A", "00_analyze_task"],
    );
    assert!(err.contains("no active lease owned by worker for step"));

    let _ = fs::remove_file(&db);
}

#[test]
fn retryable_failure_returns_step_to_pending() {
    let db = unique_db_path("retryable_failure_returns_step_to_pending");
    setup_task(&db, "task_retry");

    run(&db, &["schedule", "task_retry"]);
    run(&db, &["claim-worker", "task_retry", "worker-A"]);
    run(
        &db,
        &["start-step", "task_retry", "worker-A", "00_analyze_task"],
    );
    run(
        &db,
        &[
            "fail-step",
            "task_retry",
            "worker-A",
            "00_analyze_task",
            "retry: network blip",
        ],
    );
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
    run(
        &db,
        &["start-step", "task_blocked", "worker-A", "00_analyze_task"],
    );
    run(
        &db,
        &[
            "fail-step",
            "task_blocked",
            "worker-A",
            "00_analyze_task",
            "blocked: waiting_on dependency",
        ],
    );
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
fn cli_compile_error_routes_to_codefix_flow() {
    let out = Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--bin",
            "deterministic_ai_kernel",
            "--",
            "plan-task",
            "--compile-error",
            "dummy.log",
        ])
        .output()
        .expect("failed to run cargo plan-task");

    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("read repository"),
        "missing step: {}",
        stdout
    );
    assert!(stdout.contains("locate bug"), "missing step: {}", stdout);
    assert!(stdout.contains("patch code"), "missing step: {}", stdout);
    assert!(stdout.contains("run tests"), "missing step: {}", stdout);
    assert!(
        stdout.contains("validate patch"),
        "missing step: {}",
        stdout
    );
}

#[test]
fn scheduler_treats_all_codefix_sources_the_same() {
    use deterministic_ai_kernel::workflow::compiler::{TaskInput, Workflow};

    let compile_steps = Workflow::build_steps(&TaskInput::from_compile_error("cargo-check.log"));
    let test_steps =
        Workflow::build_steps(&TaskInput::from_test_failure("scheduler_integration.log"));
    let lint_steps = Workflow::build_steps(&TaskInput::from_lint_report("clippy.log"));

    assert_eq!(compile_steps, test_steps);
    assert_eq!(test_steps, lint_steps);
}

#[test]
fn codefix_runtime_flow_uses_lease_backed_claim_and_execution() {
    let task_id = "task-codefix-runtime";
    let db = unique_db_path(task_id);
    setup_codefix_task(&db, task_id);

    run(&db, &["schedule", task_id]);
    run(&db, &["claim-worker", task_id, "worker-plan"]);
    run(
        &db,
        &["start-step", task_id, "worker-plan", "00_read_repository"],
    );
    run(
        &db,
        &[
            "complete-step",
            task_id,
            "worker-plan",
            "00_read_repository",
        ],
    );

    run(&db, &["schedule", task_id]);
    let out = run(&db, &["claim-worker", task_id, "worker-exec"]);
    assert!(
        out.contains("STEP_CLAIMED: 01_locate_bug"),
        "unexpected claim output: {}",
        out
    );

    let out = run(
        &db,
        &["start-step", task_id, "worker-exec", "01_locate_bug"],
    );
    assert!(
        out.contains("STEP_RUNNING_OK"),
        "unexpected output: {}",
        out
    );
}

#[test]
fn start_step_accepts_generic_worker_and_planner_worker() {
    let task_id = "task-claim-cap";
    let db = unique_db_path(task_id);
    setup_codefix_task(&db, task_id);

    run(&db, &["schedule", task_id]);
    run(&db, &["claim-worker", task_id, "worker-exec"]);
    let out = run(
        &db,
        &["start-step", task_id, "worker-exec", "00_read_repository"],
    );
    assert!(
        out.contains("STEP_RUNNING_OK"),
        "unexpected output: {}",
        out
    );

    let db2 = unique_db_path("task-claim-cap-ok");
    setup_codefix_task(&db2, task_id);
    run(&db2, &["schedule", task_id]);
    run(&db2, &["claim-worker", task_id, "worker-plan"]);
    let out = run(
        &db2,
        &["start-step", task_id, "worker-plan", "00_read_repository"],
    );
    assert!(
        out.contains("STEP_RUNNING_OK"),
        "unexpected output: {}",
        out
    );
}

#[test]
fn unknown_task_class_is_rejected() {
    let db = unique_db_path("unknown_task_class_is_rejected");
    setup_task(&db, "task_bad_class");

    sqlite(
        &db,
        "UPDATE tasks SET task_class = 'Bogus' WHERE task_id = 'task_bad_class';",
    );

    let err = run_expect_fail(&db, &["schedule", "task_bad_class"]);
    assert!(
        err.contains("unknown task_class") || err.contains("missing task_class"),
        "expected explicit task_class validation failure, got: {}",
        err
    );

    let _ = fs::remove_file(&db);
}

#[test]
fn double_commit_is_rejected_after_lease_reclaim() {
    let db = unique_db_path("double_commit_is_rejected_after_lease_reclaim");
    setup_task(&db, "task_double_commit");

    run(&db, &["schedule", "task_double_commit"]);
    run(&db, &["claim-worker", "task_double_commit", "worker-A"]);
    run(
        &db,
        &[
            "start-step",
            "task_double_commit",
            "worker-A",
            "00_analyze_task",
        ],
    );

    run(&db, &["expire-leases", "task_double_commit"]);
    run(&db, &["reconcile", "task_double_commit"]);
    run(&db, &["schedule", "task_double_commit"]);
    run(&db, &["claim-worker", "task_double_commit", "worker-B"]);
    run(
        &db,
        &[
            "start-step",
            "task_double_commit",
            "worker-B",
            "00_analyze_task",
        ],
    );

    let stale = run_expect_fail(
        &db,
        &[
            "complete-step",
            "task_double_commit",
            "worker-A",
            "00_analyze_task",
        ],
    );
    assert!(
        stale.contains("no active lease owned by worker for step"),
        "{stale}"
    );

    let fresh = run(
        &db,
        &[
            "complete-step",
            "task_double_commit",
            "worker-B",
            "00_analyze_task",
        ],
    );
    assert!(fresh.contains("COMPLETE"), "{fresh}");

    let double = run_expect_fail(
        &db,
        &[
            "complete-step",
            "task_double_commit",
            "worker-B",
            "00_analyze_task",
        ],
    );
    assert!(
        double.contains("no active lease owned by worker for step")
            || double.contains("already")
            || double.contains("committed"),
        "{double}"
    );
}

fn query_status(db: &std::path::Path, task_id: &str, step_id: &str) -> String {
    let out = std::process::Command::new("sqlite3")
        .arg(db)
        .arg(format!(
            "SELECT status FROM step_status WHERE task_id='{}' AND step_id='{}'",
            task_id, step_id
        ))
        .output()
        .expect("sqlite3 failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn complete_step_unlocks_dependent_steps() {
    let db = unique_db_path("complete_step_unlocks_dependent_steps");
    setup_task(&db, "task_unlock");

    run(&db, &["schedule", "task_unlock"]);
    assert_eq!(
        query_status(&db, "task_unlock", "00_analyze_task"),
        "dispatched"
    );
    assert_eq!(
        query_status(&db, "task_unlock", "01_plan_execution"),
        "pending"
    );
    assert_eq!(
        query_status(&db, "task_unlock", "02_execute_changes"),
        "pending"
    );

    run(&db, &["claim-worker", "task_unlock", "worker-A"]);
    run(
        &db,
        &["start-step", "task_unlock", "worker-A", "00_analyze_task"],
    );
    let out = run(
        &db,
        &[
            "complete-step",
            "task_unlock",
            "worker-A",
            "00_analyze_task",
        ],
    );
    assert!(out.contains("STEP_COMPLETE_OK"), "{out}");

    assert_eq!(
        query_status(&db, "task_unlock", "00_analyze_task"),
        "committed"
    );
    assert_ne!(
        query_status(&db, "task_unlock", "01_plan_execution"),
        "pending",
        "dependent step must be unblocked after predecessor completes"
    );
    assert_eq!(
        query_status(&db, "task_unlock", "02_execute_changes"),
        "pending"
    );

    let _ = std::fs::remove_file(&db);
}

#[test]
fn terminal_failure_rejects_step() {
    let db = unique_db_path("terminal_failure_rejects_step");
    setup_task(&db, "task_terminal");

    run(&db, &["schedule", "task_terminal"]);
    run(&db, &["claim-worker", "task_terminal", "worker-A"]);
    run(
        &db,
        &["start-step", "task_terminal", "worker-A", "00_analyze_task"],
    );

    let out = run(
        &db,
        &[
            "fail-step",
            "task_terminal",
            "worker-A",
            "00_analyze_task",
            "fatal: unrecoverable error",
        ],
    );
    assert!(out.contains("STEP_FAIL_OK"), "{out}");

    assert_eq!(
        query_status(&db, "task_terminal", "00_analyze_task"),
        "rejected",
        "terminal failure must set step to rejected"
    );

    let _ = std::fs::remove_file(&db);
}
