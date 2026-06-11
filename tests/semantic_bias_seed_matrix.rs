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

fn emit_and_fetch(db: &str, task: &str, step: &str, steps: &[&str]) -> Value {
    let mut args = vec!["emit-bias-artifact", task, step];
    args.extend_from_slice(steps);

    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(&args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "emit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let fetch = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(["latest-bias-artifact", task, step])
        .output()
        .unwrap();
    assert!(fetch.status.success());

    let stdout = String::from_utf8(fetch.stdout).unwrap();
    let line = stdout.lines().next().expect("expected artifact row");
    let cols: Vec<&str> = line.splitn(6, '\t').collect();
    serde_json::from_str(cols[5]).unwrap()
}

#[test]
fn semantic_bias_same_steps_same_payload_across_task_ids() {
    let step_sets: &[&[&str]] = &[
        &["AnalyzeTask"],
        &["ExecuteChanges"],
        &["RunTests"],
        &["AnalyzeTask", "ExecuteChanges"],
        &["AnalyzeTask", "RunTests"],
        &["ExecuteChanges", "RunTests"],
        &["AnalyzeTask", "ExecuteChanges", "RunTests"],
        &["RunTests", "AnalyzeTask", "ExecuteChanges"],
    ];

    for (i, steps) in step_sets.iter().enumerate() {
        let task = format!("matrix-task-{i}");
        let step = format!("matrix-step-{i}");

        let db1 = unique_db(&format!("seed_matrix_{i}_a"));
        let db2 = unique_db(&format!("seed_matrix_{i}_b"));
        cleanup(&db1);
        cleanup(&db2);

        let p1 = emit_and_fetch(&db1, &task, &step, steps);
        let p2 = emit_and_fetch(&db2, &task, &step, steps);

        assert_eq!(p1, p2, "drift detected for step_set={i} steps={steps:?}");
        assert_eq!(p1["version"], 1, "version must be 1 for step_set={i}");

        cleanup(&db1);
        cleanup(&db2);
    }
}
