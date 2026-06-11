use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
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
        .unwrap();

    assert!(
        out.status.success(),
        "command failed: {:?}\nstdout=\n{}\nstderr=\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    String::from_utf8(out.stdout).unwrap()
}

fn run_fail(db: &PathBuf, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .unwrap();

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
    let json_line = out
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .expect("expected restore payload json");
    serde_json::from_str(json_line).unwrap()
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
    let db = unique_db("event_ordering_invalid_causal_sequence");
    cleanup(&db);

    let err = run_fail(&db, &["schedule", "invalid-order-task"]);
    assert!(!err.trim().is_empty(), "expected non-empty failure output");

    cleanup(&db);
}
