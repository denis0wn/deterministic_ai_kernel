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

fn latest_payload(db: &str, task: &str, step: &str) -> Value {
    let stdout = run(db, &["latest-bias-artifact", task, step]);
    let line = stdout.lines().next().expect("expected artifact row");
    let cols: Vec<&str> = line.splitn(6, '\t').collect();
    serde_json::from_str(cols[5]).unwrap()
}

fn long_chain_steps() -> Vec<&'static str> {
    vec![
        "AnalyzeTask",
        "ExecuteChanges",
        "RunTests",
        "AnalyzeTask",
        "ExecuteChanges",
        "RunTests",
        "AnalyzeTask",
        "ExecuteChanges",
        "RunTests",
    ]
}

#[test]
fn replay_long_chain_matches_after_snapshot_restore() {
    let cases = [
        ("task-chain-0", "step-chain-0"),
        ("task-chain-1", "step-chain-1"),
        ("task-chain-42", "step-chain-42"),
        ("task-chain-123", "step-chain-123"),
        ("task-chain-999", "step-chain-999"),
    ];

    for (task, step) in cases {
        let db = unique_db(task);
        cleanup(&db);

        let steps = long_chain_steps();
        let args: Vec<&str> = std::iter::once("emit-bias-artifact")
            .chain(std::iter::once(task))
            .chain(std::iter::once(step))
            .chain(steps.iter().copied())
            .collect();

        let _ = run(&db, &args);
        let direct = latest_payload(&db, task, step);

        let _ = run(&db, &["snapshot", task]);
        let _ = run(&db, &["restore", task]);
        let after_restore = latest_payload(&db, task, step);

        assert_eq!(
            direct, after_restore,
            "replay drift for task={task} step={step}"
        );
        assert_eq!(direct["version"], "v1");
        assert_eq!(direct["seed"], 0);

        cleanup(&db);
    }
}
