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

fn restore_payload(db: &PathBuf, task_id: &str) -> Value {
    let out = run_ok(db, &["restore", task_id]);
    let json_line = out
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("expected restore payload json");
    serde_json::from_str(json_line).expect("test failure")
}

#[test]
fn semantic_bias_permutations_keep_replay_deterministic() {
    let orders = [
        vec!["AnalyzeTask", "ExecuteChanges", "RunTests"],
        vec!["AnalyzeTask", "RunTests", "ExecuteChanges"],
        vec!["ExecuteChanges", "AnalyzeTask", "RunTests"],
    ];

    let mut baseline_payload: Option<Value> = None;

    for (idx, order) in orders.iter().enumerate() {
        let db = unique_db(&format!("event_ordering_semantic_bias_{idx}"));
        cleanup(&db);

        let _ = run_ok(
            &db,
            &[
                "emit-bias-artifact",
                "fuzz-task",
                "fuzz-step",
                order[0],
                order[1],
                order[2],
            ],
        );

        let replay = run_ok(&db, &["replay", "fuzz-task"]);
        assert!(replay.contains("REPLAY OK"), "{replay}");
        assert!(replay.contains("true"), "{replay}");

        let _ = run_ok(&db, &["snapshot", "fuzz-task"]);
        let payload = restore_payload(&db, "fuzz-task");

        assert_eq!(payload["task_id"], "fuzz-task");
        assert!(payload["artifacts"]["semantic_bias_v1"].is_number());

        if let Some(baseline) = &baseline_payload {
            assert_eq!(payload["task_id"], baseline["task_id"]);
            assert_eq!(payload["done"], baseline["done"]);
            assert_eq!(payload["artifacts"], baseline["artifacts"]);
        } else {
            baseline_payload = Some(payload);
        }

        cleanup(&db);
    }
}

#[test]
fn invalid_causal_sequence_fails_deterministically() {
    // Regression (audit findings C1/M9 + "fake ordering tests"): malformed
    // event streams must be rejected by the replay validator. The previous
    // version of this test created NO events and asserted only that an
    // unrelated command printed something — it never exercised ordering.
    let db = unique_db("event_ordering_invalid_causal_sequence");
    cleanup(&db);

    // Real production events first: submit + schedule dispatches step 00.
    let _ = run_ok(&db, &["submit-task", "t1"]);
    let _ = run_ok(&db, &["schedule", "t1"]);

    // Sanity: a clean scheduler-produced lifecycle validates.
    let clean = run_ok(&db, &["replay", "t1"]);
    assert!(clean.contains("REPLAY OK: true"), "{clean}");

    // Inject malformed units directly into the event log.
    let sql = r#"
        -- unit 9000: STEP_COMPLETED sequenced BEFORE STEP_STARTED
        INSERT INTO event_log
            (task_id, causal_unit_id, sequence_in_unit, event_type, payload,
             system_generation, logical_generation, step_id)
        VALUES
            ('t1', 9000, 0, 'STEP_COMPLETED',
             '{"step_id":"00_analyze_task","outcome":"Success"}',
             9000, 9000, '00_analyze_task'),
            ('t1', 9000, 1, 'STEP_STARTED',
             '{"step_id":"00_analyze_task","worker_id":"w"}',
             9000, 9000, '00_analyze_task');
        -- unit 9001: sequence gap (0 then 2)
        INSERT INTO event_log
            (task_id, causal_unit_id, sequence_in_unit, event_type, payload,
             system_generation, logical_generation, step_id)
        VALUES
            ('t1', 9001, 0, 'STEP_STARTED',
             '{"step_id":"01_plan_execution","worker_id":"w"}',
             9001, 9001, '01_plan_execution'),
            ('t1', 9001, 2, 'STEP_COMPLETED',
             '{"step_id":"01_plan_execution","outcome":"Success"}',
             9001, 9001, '01_plan_execution');
    "#;
    let status = std::process::Command::new("sqlite3")
        .arg(&db)
        .arg(sql)
        .status()
        .expect("sqlite3 must be available");
    assert!(status.success(), "event injection failed");

    // The validator must now reject the task, citing concrete violations.
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", &db)
        .args(["replay", "t1"])
        .output()
        .expect("test failure");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("REPLAY OK: false"), "{combined}");
    assert!(combined.contains("INVALID"), "{combined}");

    cleanup(&db);
}
