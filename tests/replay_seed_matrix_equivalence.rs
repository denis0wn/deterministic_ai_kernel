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

fn seed_matrix() -> Vec<(&'static str, &'static str, Vec<&'static str>)> {
    vec![
        ("matrix-task-0", "matrix-step-0", vec!["AnalyzeTask"]),
        ("matrix-task-1", "matrix-step-1", vec!["ExecuteChanges"]),
        ("matrix-task-2", "matrix-step-2", vec!["RunTests"]),
        (
            "matrix-task-3",
            "matrix-step-3",
            vec!["AnalyzeTask", "ExecuteChanges"],
        ),
        (
            "matrix-task-4",
            "matrix-step-4",
            vec!["AnalyzeTask", "RunTests"],
        ),
        (
            "matrix-task-5",
            "matrix-step-5",
            vec!["ExecuteChanges", "RunTests"],
        ),
        (
            "matrix-task-6",
            "matrix-step-6",
            vec!["AnalyzeTask", "ExecuteChanges", "RunTests"],
        ),
        (
            "matrix-task-7",
            "matrix-step-7",
            vec!["RunTests", "AnalyzeTask", "ExecuteChanges"],
        ),
    ]
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
fn seed_matrix_replay_equivalence_matches_after_snapshot_restore() {
    for (task, step, steps) in seed_matrix() {
        let db = unique_db(task);
        cleanup(&db);

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

        assert_eq!(direct, after_restore, "drift for task={task} step={step}");
        assert_eq!(direct["version"], "v1");
        assert_eq!(direct["seed"], 0);

        cleanup(&db);
    }
}

#[test]
fn seed_matrix_long_chain_replay_equivalence_matches_after_snapshot_restore() {
    for i in [0, 1, 7, 42, 123, 999, 1024, 65535] {
        let task = format!("long-task-{i}");
        let step = format!("long-step-{i}");
        let db = unique_db(&task);
        cleanup(&db);

        let steps = long_chain_steps();
        let args: Vec<&str> = std::iter::once("emit-bias-artifact")
            .chain(std::iter::once(task.as_str()))
            .chain(std::iter::once(step.as_str()))
            .chain(steps.iter().copied())
            .collect();

        let _ = run(&db, &args);
        let direct = latest_payload(&db, &task, &step);

        let _ = run(&db, &["snapshot", &task]);
        let _ = run(&db, &["restore", &task]);
        let after_restore = latest_payload(&db, &task, &step);

        assert_eq!(direct, after_restore, "drift for task={task} step={step}");
        assert_eq!(direct["version"], "v1");
        assert_eq!(direct["seed"], 0);

        cleanup(&db);
    }
}
