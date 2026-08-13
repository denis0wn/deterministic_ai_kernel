//! Canonical kernel scenario suite (audit remediation R13).
//!
//! Every scenario drives the REAL production scheduler/worker/event path via
//! the compiled kernel binary against disposable databases — no fabricated
//! state, no mocked kernel internals. These scenarios are the regression
//! contract for the canonical state machine:
//!
//!   A  successful scheduler lifecycle      -> replay VALID
//!   B  retryable failure                  -> live == reconcile == snapshot
//!   C  terminal failure                   -> task FAILED, rerun != success
//!   D  lease expiry -> redispatch         -> eventual commit, replay VALID
//!   E  malformed ordering                 -> replay INVALID
//!   F  pipeline events survive prior worker activity
//!   G  CodeFix ExecSpec survives planner -> scheduler
//!   H  provider/LLM error                -> retryable, never terminal-by-default
//!   I  primitive hashing is pure (no side effects)
//!   J  integrity-json cannot destroy the database
//!   K  fresh-DB CLI matrix contains zero panics
//!   L  concurrent heartbeats -> no silent event loss

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("dak_scenario_{}_{}.db", name, nanos))
}

fn cleanup(db: &PathBuf) {
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_file(format!("{}-wal", db.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db.display()));
}

struct Cli {
    db: PathBuf,
}

impl Cli {
    fn new(db: &PathBuf) -> Self {
        Self { db: db.clone() }
    }

    fn run(&self, args: &[&str]) -> (bool, String, String) {
        self.run_env(args, &[])
    }

    fn run_env(&self, args: &[&str], extra_env: &[(&str, &str)]) -> (bool, String, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"));
        cmd.env("KERNEL_DB_PATH", &self.db);
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let out = cmd.args(args).output().expect("spawn kernel binary");
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn ok(&self, args: &[&str]) -> String {
        let (success, stdout, stderr) = self.run(args);
        assert!(
            success,
            "command {:?} failed\nstdout={}\nstderr={}",
            args, stdout, stderr
        );
        assert!(
            !stderr.contains("panicked"),
            "command {:?} panicked: {}",
            args,
            stderr
        );
        stdout
    }
}

fn sql(db: &PathBuf, query: &str) -> String {
    let out = Command::new("sqlite3")
        .arg(db)
        .arg(query)
        .output()
        .expect("sqlite3 available");
    assert!(out.status.success(), "sqlite3 query failed: {query}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn complete_step(cli: &Cli, task: &str, worker: &str, step: &str) {
    cli.ok(&["claim-worker", task, worker]);
    cli.ok(&["start-step", task, worker, step]);
    cli.ok(&["complete-step", task, worker, step]);
    cli.ok(&["schedule", task]);
}

// ── Scenario A ───────────────────────────────────────────────────────────────

#[test]
fn scenario_a_successful_lifecycle_replays_valid() {
    let db = unique_db("a_success");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "taskA"]);
    cli.ok(&["schedule", "taskA"]);
    complete_step(&cli, "taskA", "worker-planner", "00_analyze_task");
    complete_step(&cli, "taskA", "worker-planner", "01_plan_execution");
    complete_step(&cli, "taskA", "worker-executor", "02_execute_changes");

    let statuses = sql(
        &db,
        "SELECT DISTINCT status FROM step_status WHERE task_id='taskA'",
    );
    assert_eq!(statuses, "committed");

    let replay = cli.ok(&["replay", "taskA"]);
    assert!(replay.contains("REPLAY OK: true"), "{replay}");

    cleanup(&db);
}

// ── Scenario B ───────────────────────────────────────────────────────────────

#[test]
fn scenario_b_retryable_failure_keeps_live_replay_snapshot_consistent() {
    let db = unique_db("b_retryable");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "taskB"]);
    cli.ok(&["schedule", "taskB"]);
    cli.ok(&["claim-worker", "taskB", "worker-planner"]);
    cli.ok(&["start-step", "taskB", "worker-planner", "00_analyze_task"]);
    cli.ok(&[
        "fail-step",
        "taskB",
        "worker-planner",
        "00_analyze_task",
        "retry: transient blip",
    ]);

    // Live state: the retryable step must NOT be rejected.
    let live = sql(
        &db,
        "SELECT step_id || '=' || status FROM step_status WHERE task_id='taskB' ORDER BY step_id",
    );
    assert!(live.contains("00_analyze_task=pending"), "{live}");
    assert!(!live.contains("rejected"), "{live}");

    // Reconcile (event fold -> state tables) keeps the step retryable. The
    // dependency-free root step is additionally promoted to `ready` by the
    // unlock pass — the invariant is that it is never `rejected` and stays
    // re-dispatchable (audit finding C2 regression). The reconcile CLI emits
    // a JSON status map: STEP_STATUS: {"<step>": "<status>", ...}.
    let reconcile = cli.ok(&["reconcile", "taskB"]);
    assert!(
        reconcile.contains("\"00_analyze_task\": \"ready\"")
            || reconcile.contains("\"00_analyze_task\": \"pending\""),
        "{reconcile}"
    );
    assert!(!reconcile.contains("rejected"), "{reconcile}");

    // Snapshot reconstruction must agree on the essential state: every step
    // is present and the retryable step is NOT rejected (it may be recorded
    // as the raw fold state `pending`; the scheduler unlock pass promotes it
    // to `ready` on the next cycle, so both converge).
    cli.ok(&["snapshot", "taskB"]);
    let steps_json = sql(
        &db,
        "SELECT json_extract(payload, '$.steps') FROM state_snapshots \
         WHERE task_id='taskB' ORDER BY snapshot_id DESC LIMIT 1",
    );
    let steps: serde_json::Value = serde_json::from_str(&steps_json).expect("steps json");
    for step in ["00_analyze_task", "01_plan_execution", "02_execute_changes"] {
        let st = steps[step].as_str().unwrap_or("<missing>");
        assert!(
            matches!(st, "pending" | "ready"),
            "step {step} must stay retryable, got {st} ({steps_json})"
        );
    }
    assert_eq!(steps["01_plan_execution"], "pending", "{steps_json}");
    assert_eq!(steps["02_execute_changes"], "pending", "{steps_json}");

    let replay = cli.ok(&["replay", "taskB"]);
    assert!(replay.contains("REPLAY OK: true"), "{replay}");

    cleanup(&db);
}

// ── Scenario C ───────────────────────────────────────────────────────────────

#[test]
fn scenario_c_terminal_failure_is_not_success() {
    let db = unique_db("c_terminal");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "taskC"]);
    cli.ok(&["schedule", "taskC"]);
    cli.ok(&["claim-worker", "taskC", "worker-planner"]);
    cli.ok(&["start-step", "taskC", "worker-planner", "00_analyze_task"]);
    cli.ok(&[
        "fail-step",
        "taskC",
        "worker-planner",
        "00_analyze_task",
        "fatal: unrecoverable",
    ]);

    let live = sql(
        &db,
        "SELECT status FROM step_status WHERE task_id='taskC' AND step_id='00_analyze_task'",
    );
    assert_eq!(live, "rejected");

    // execute-effects must NOT report success for a terminally failed task
    // (audit finding C4 regression): non-zero exit, no EFFECT_EXECUTION_OK.
    let (success, stdout, stderr) = cli.run(&["execute-effects", "taskC"]);
    assert!(!success, "terminal-failed task must not exit 0");
    assert!(!stdout.contains("EFFECT_EXECUTION_OK"), "{stdout}");
    assert!(
        stderr.contains("failed") || stdout.contains("failed"),
        "stdout={stdout} stderr={stderr}"
    );

    // Re-run must stay a failure — no fake recovery into success.
    let (success2, stdout2, _) = cli.run(&["execute-effects", "taskC"]);
    assert!(!success2, "rerun of terminal-failed task must not exit 0");
    assert!(!stdout2.contains("EFFECT_EXECUTION_OK"), "{stdout2}");

    cleanup(&db);
}

// ── Scenario D ───────────────────────────────────────────────────────────────

#[test]
fn scenario_d_lease_expiry_redispatch_eventual_commit() {
    let db = unique_db("d_expiry");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "taskD"]);
    cli.ok(&["schedule", "taskD"]);

    // Force the dispatched lease to be overdue, then expire it atomically.
    sql(
        &db,
        "UPDATE leases SET expires_at_generation = 0 WHERE task_id='taskD' AND state='active'",
    );
    let expire_out = cli.ok(&["expire-leases", "taskD"]);
    assert!(expire_out.contains("EXPIRED_LEASES: 1"), "{expire_out}");

    // The event log carries the expiry (durability contract: no lease state
    // change without an event — audit finding H7 regression).
    let expired_events = sql(
        &db,
        "SELECT COUNT(*) FROM event_log WHERE task_id='taskD' AND event_type='LEASE_EXPIRED'",
    );
    assert_eq!(expired_events, "1");

    // Recovery: schedule redispatches the step; a new worker completes it.
    cli.ok(&["schedule", "taskD"]);
    complete_step(&cli, "taskD", "worker-planner", "00_analyze_task");
    complete_step(&cli, "taskD", "worker-planner", "01_plan_execution");
    complete_step(&cli, "taskD", "worker-executor", "02_execute_changes");

    let replay = cli.ok(&["replay", "taskD"]);
    assert!(replay.contains("REPLAY OK: true"), "{replay}");

    cleanup(&db);
}

// ── Scenario E ───────────────────────────────────────────────────────────────

#[test]
fn scenario_e_malformed_ordering_is_invalid() {
    let db = unique_db("e_malformed");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "taskE"]);
    cli.ok(&["schedule", "taskE"]);

    sql(
        &db,
        "INSERT INTO event_log \
            (task_id, causal_unit_id, sequence_in_unit, event_type, payload, \
             system_generation, logical_generation, step_id) \
         VALUES \
            ('taskE', 9500, 0, 'STEP_COMPLETED', \
             '{\"step_id\":\"00_analyze_task\",\"outcome\":\"Success\"}', \
             9500, 9500, '00_analyze_task'), \
            ('taskE', 9500, 1, 'STEP_STARTED', \
             '{\"step_id\":\"00_analyze_task\",\"worker_id\":\"w\"}', \
             9500, 9500, '00_analyze_task')",
    );

    let (success, stdout, stderr) = cli.run(&["replay", "taskE"]);
    // The replay command itself succeeds (it is a report), but the verdict
    // must be INVALID with concrete violations.
    let combined = format!("{stdout}{stderr}");
    let _ = success;
    assert!(combined.contains("REPLAY OK: false"), "{combined}");
    assert!(combined.contains("INVALID"), "{combined}");

    cleanup(&db);
}

// ── Scenario F ───────────────────────────────────────────────────────────────

#[test]
fn scenario_f_pipeline_events_survive_prior_worker_activity() {
    let db = unique_db("f_pipeline");
    cleanup(&db);
    let cli = Cli::new(&db);

    // Worker activity first (allocates causal units via the unified clock).
    cli.ok(&["submit-task", "taskF"]);
    cli.ok(&["schedule", "taskF"]);

    // Two pipeline publications AFTER worker activity must both be durable
    // (audit finding C7 regression: previously the second publish was
    // silently dropped by a causal-unit allocator collision).
    let (s1, _, e1) = cli.run(&[
        "pipeline-run",
        "--payload",
        "first durable publish",
        "--seed",
        "11",
    ]);
    // Execution may fail (no LLM), but publication happens before execution.
    let _ = (s1, e1);
    let (s2, _, e2) = cli.run(&[
        "pipeline-run",
        "--payload",
        "second durable publish",
        "--seed",
        "12",
    ]);
    let _ = (s2, e2);

    let pipeline_events = sql(
        &db,
        "SELECT COUNT(*) FROM event_log WHERE event_type LIKE 'pipeline.%'",
    );
    assert_eq!(
        pipeline_events, "14",
        "both pipeline publications must be durable (7 events each)"
    );

    // No duplicate causal units (single allocator contract).
    let dup_units = sql(
        &db,
        "SELECT COUNT(*) FROM (SELECT causal_unit_id, sequence_in_unit \
         FROM event_log GROUP BY causal_unit_id, sequence_in_unit \
         HAVING COUNT(*) > 1)",
    );
    assert_eq!(dup_units, "0");

    cleanup(&db);
}

// ── Scenario G ───────────────────────────────────────────────────────────────

#[test]
fn scenario_g_codefix_spec_survives_planner_to_scheduler() {
    let db = unique_db("g_codefix");
    cleanup(&db);
    let cli = Cli::new(&db);
    // Prime schema (plan-task opens the DB via the initialized path).
    let _ = cli.run(&["schedule", "prime"]);

    let out = cli.ok(&[
        "plan-task",
        "--compile-error",
        "error[E0308]: mismatched types at src/main.rs:42",
    ]);
    assert!(out.contains("TASK_CLASS: CodeFix"), "{out}");
    let task_id = out
        .lines()
        .find(|l| l.starts_with("TASK_ID:"))
        .expect("TASK_ID printed")
        .trim_start_matches("TASK_ID:")
        .trim()
        .to_string();

    // Persisted spec must be the CodeFix graph, not a regenerated default.
    let spec_steps = sql(
        &db,
        &format!(
            "SELECT json_extract(exec_spec, '$.steps[0].step_id') FROM tasks WHERE task_id='{task_id}'"
        ),
    );
    assert_eq!(spec_steps, "00_read_repository");

    // Scheduler must dispatch exactly the planned step.
    cli.ok(&["schedule", &task_id]);
    let dispatched = sql(
        &db,
        &format!(
            "SELECT step_id FROM step_status WHERE task_id='{task_id}' AND status='dispatched'"
        ),
    );
    assert_eq!(dispatched, "00_read_repository");

    // Worker flow on the CodeFix graph.
    cli.ok(&["claim-worker", &task_id, "worker-planner"]);
    cli.ok(&[
        "start-step",
        &task_id,
        "worker-planner",
        "00_read_repository",
    ]);
    cli.ok(&[
        "complete-step",
        &task_id,
        "worker-planner",
        "00_read_repository",
    ]);

    let replay = cli.ok(&["replay", &task_id]);
    assert!(replay.contains("REPLAY OK: true"), "{replay}");

    cleanup(&db);
}

// ── Scenario H ───────────────────────────────────────────────────────────────

#[test]
fn scenario_h_provider_errors_are_retryable_not_terminal() {
    let db = unique_db("h_provider_error");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "taskH"]);
    cli.ok(&["schedule", "taskH"]);
    cli.ok(&["claim-worker", "taskH", "worker-planner"]);
    cli.ok(&["start-step", "taskH", "worker-planner", "00_analyze_task"]);
    cli.ok(&[
        "fail-step",
        "taskH",
        "worker-planner",
        "00_analyze_task",
        "primitive_execution_error: mlx request failed: connection refused",
    ]);

    // A provider outage must not permanently reject the step (audit finding
    // C4 regression): the step returns to the retryable pool.
    let live = sql(
        &db,
        "SELECT status FROM step_status WHERE task_id='taskH' AND step_id='00_analyze_task'",
    );
    assert_ne!(live, "rejected", "provider error must not be terminal");
    assert!(live == "pending" || live == "ready", "{live}");

    // execute-effects reports incomplete/failed, never success.
    //
    // P0 MLX lifecycle note: this scenario asserts the audit-C4 contract
    // "provider outage is retryable, never terminal". It must be fully
    // deterministic, so the provider is pointed at a guaranteed-dead port
    // (discard port 9) instead of relying on whatever may or may not be
    // listening on the .env port, and lifecycle management is disabled so
    // the kernel does NOT auto-start a server there (auto-restart is the
    // desired production recovery, but this scenario locks the genuine
    // connection-refused failure path). The analyze step's embedding call
    // then fails on the dead endpoint regardless of free memory.
    let (success, stdout, _) = cli.run_env(
        &["execute-effects", "taskH"],
        &[
            ("MLX_LIFECYCLE", "off"),
            ("OPENAI_BASE_URL", "http://127.0.0.1:9/v1"),
        ],
    );
    assert!(!success);
    assert!(!stdout.contains("EFFECT_EXECUTION_OK"), "{stdout}");

    cleanup(&db);
}

// ── Scenario I ───────────────────────────────────────────────────────────────

#[test]
fn scenario_i_primitive_hashing_is_pure() {
    let db = unique_db("i_purity");
    cleanup(&db);

    let probe_cmd = format!(
        "touch {}/dek_scenario_i_probe.txt",
        std::env::temp_dir().display()
    );
    let probe_file = std::env::temp_dir().join("dek_scenario_i_probe.txt");
    let _ = std::fs::remove_file(&probe_file);

    let out = Command::new(env!("CARGO_BIN_EXE_run"))
        .env("KERNEL_DB_PATH", &db)
        .args([
            "run",
            "--payload",
            &format!("run command {probe_cmd}"),
            "--seed",
            "7",
        ])
        .output()
        .expect("spawn run binary");
    let _ = out.status; // execution success is irrelevant to purity

    assert!(
        !probe_file.exists(),
        "hashing a command primitive must not execute the command"
    );

    let _ = std::fs::remove_file(&probe_file);
    cleanup(&db);
}

// ── Scenario J ───────────────────────────────────────────────────────────────

#[test]
fn scenario_j_integrity_json_cannot_destroy_data() {
    let db = unique_db("j_integrity");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "keepme"]);
    cli.ok(&["schedule", "keepme"]);

    let tasks_before = sql(&db, "SELECT COUNT(*) FROM tasks");
    let events_before = sql(&db, "SELECT COUNT(*) FROM event_log");

    let (success, _, _) = cli.run(&["integrity-json"]);
    assert!(success, "integrity-json must succeed");

    let tasks_after = sql(&db, "SELECT COUNT(*) FROM tasks");
    let events_after = sql(&db, "SELECT COUNT(*) FROM event_log");
    assert_eq!(
        tasks_before, tasks_after,
        "integrity-json must not delete tasks"
    );
    assert_eq!(
        events_before, events_after,
        "integrity-json must not delete events"
    );

    cleanup(&db);
}

// ── Scenario K ───────────────────────────────────────────────────────────────

#[test]
fn scenario_k_fresh_db_cli_matrix_has_zero_panics() {
    let commands: Vec<Vec<&str>> = vec![
        vec!["submit-task", "t1"],
        vec!["schedule", "t1"],
        vec!["replay", "t1"],
        vec!["reconcile", "t1"],
        vec!["status-map", "t1"],
        vec!["snapshot", "t1"],
        vec!["restore", "t1"],
        vec!["stats", "t1"],
        vec!["next-ready", "t1"],
        vec!["claim-worker", "t1", "worker-planner"],
        vec!["start-step", "t1", "worker-planner", "00_analyze_task"],
        vec!["heartbeat", "t1", "worker-planner", "00_analyze_task"],
        vec!["complete-step", "t1", "worker-planner", "00_analyze_task"],
        vec![
            "fail-step",
            "t1",
            "worker-planner",
            "00_analyze_task",
            "retry: x",
        ],
        vec!["expire-leases", "t1"],
        vec!["execute-effects", "t1"],
        vec!["integrity-json"],
        vec!["capture-capsule", "t1"],
        vec!["replay-capsule", "t1"],
        vec!["latest-capsule", "t1"],
        vec!["analyze-task", "t1", "some text"],
        vec!["latest-bias-artifact", "t1"],
        vec!["reset-db"],
        vec!["doctor-json"],
        vec!["snapshot-artifacts", "t1"],
    ];

    for cmd in commands {
        let db = unique_db(&format!("k_{}", cmd[0]));
        cleanup(&db);
        let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
            .env("KERNEL_DB_PATH", &db)
            .args(&cmd)
            .output()
            .expect("spawn kernel binary");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("panicked"),
            "fresh-DB command {:?} panicked: {}",
            cmd,
            stderr
        );
        cleanup(&db);
    }
}

// ── Scenario L ───────────────────────────────────────────────────────────────

#[test]
fn scenario_l_concurrent_heartbeats_have_no_silent_loss() {
    let db = unique_db("l_concurrent");
    cleanup(&db);
    let cli = Cli::new(&db);

    cli.ok(&["submit-task", "taskL"]);
    cli.ok(&["schedule", "taskL"]);
    cli.ok(&["claim-worker", "taskL", "worker-planner"]);
    cli.ok(&["start-step", "taskL", "worker-planner", "00_analyze_task"]);

    // 24 concurrent heartbeat operations.
    let mut handles = Vec::new();
    for _ in 0..24 {
        let db_path = db.clone();
        handles.push(std::thread::spawn(move || {
            let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
                .env("KERNEL_DB_PATH", &db_path)
                .args(["heartbeat", "taskL", "worker-planner", "00_analyze_task"])
                .output()
                .expect("spawn");
            out.status.success()
        }));
    }
    let ok_count = handles
        .into_iter()
        .map(|h| h.join().expect("join"))
        .filter(|ok| *ok)
        .count();

    // Every successful heartbeat must have produced exactly one event: no
    // silent event loss, no duplicated events (audit finding M1/concurrency).
    let heartbeat_events: usize = sql(
        &db,
        "SELECT COUNT(*) FROM event_log WHERE task_id='taskL' AND event_type='WORKER_HEARTBEAT'",
    )
    .parse()
    .expect("count");
    assert_eq!(
        heartbeat_events, ok_count,
        "each successful heartbeat must persist exactly one event"
    );
    assert!(ok_count > 0, "at least one heartbeat must succeed");

    // Causal sequences stay unique under concurrent allocation. (Note: a
    // dispatch pair legitimately shares one generation — LEASE_ACQUIRED +
    // STEP_DISPATCHED are one atomic unit — so the invariant checked here is
    // the uniqueness of (causal_unit_id, sequence_in_unit) plus distinct
    // generations across independently allocated heartbeat events.)
    let dup_units = sql(
        &db,
        "SELECT COUNT(*) FROM (SELECT causal_unit_id, sequence_in_unit, COUNT(*) c \
         FROM event_log GROUP BY causal_unit_id, sequence_in_unit HAVING c > 1)",
    );
    assert_eq!(
        dup_units, "0",
        "no duplicate (causal_unit_id, sequence_in_unit)"
    );

    let dup_heartbeat_gens = sql(
        &db,
        "SELECT COUNT(*) FROM (SELECT system_generation, COUNT(*) c FROM event_log \
         WHERE event_type='WORKER_HEARTBEAT' \
         GROUP BY system_generation HAVING c > 1)",
    );
    assert_eq!(
        dup_heartbeat_gens, "0",
        "concurrently allocated heartbeat events must have distinct generations"
    );

    cleanup(&db);
}
