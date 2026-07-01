use serde_json::Value;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("deterministic_ai_kernel_{}_{}.db", name, nanos))
        .display()
        .to_string()
}

fn cleanup(db: &str) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}

fn run(db: &str, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "command failed: {:?}\nstderr=\n{}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );

    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn semantic_bias_v1_is_identical_across_repeat_emits() {
    let db = unique_db("semantic_bias_replay_equivalence");
    cleanup(&db);

    let emit_args = [
        "emit-bias-artifact",
        "task-eq",
        "step-eq",
        "AnalyzeTask",
        "ExecuteChanges",
        "RunTests",
    ];

    let _ = run(&db, &emit_args);
    let first = run(&db, &["latest-bias-artifact", "task-eq", "step-eq"]);

    let _ = run(&db, &emit_args);
    let second = run(&db, &["latest-bias-artifact", "task-eq", "step-eq"]);

    let first_line = first.lines().next().expect("expected first artifact row");
    let second_line = second.lines().next().expect("expected second artifact row");

    let first_cols: Vec<&str> = first_line.splitn(6, '\t').collect();
    let second_cols: Vec<&str> = second_line.splitn(6, '\t').collect();

    assert_eq!(first_cols[1], "task-eq");
    assert_eq!(second_cols[1], "task-eq");
    assert_eq!(first_cols[2], "step-eq");
    assert_eq!(second_cols[2], "step-eq");
    assert_eq!(first_cols[4], "semantic_bias_v1");
    assert_eq!(second_cols[4], "semantic_bias_v1");

    let first_payload: Value = serde_json::from_str(first_cols[5]).unwrap();
    let second_payload: Value = serde_json::from_str(second_cols[5]).unwrap();

    assert_eq!(
        first_payload, second_payload,
        "payload drift detected between identical runs"
    );
    assert_eq!(first_payload["version"], 1);
    assert_eq!(first_payload["seed"], 0);
    assert_eq!(
        first_payload["preferred"],
        serde_json::json!(["AnalyzeTask", "ExecuteChanges", "RunTests"])
    );

    cleanup(&db);
}
