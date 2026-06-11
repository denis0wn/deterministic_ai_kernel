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
fn snapshot_and_replay_contract_stays_compatible() {
    let db = unique_db("snapshot_and_replay_contract_stays_compatible");
    cleanup(&db);

    let _ = run(
        &db,
        &[
            "emit-bias-artifact",
            "compat-task",
            "compat-step",
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
        ],
    );

    let replay = run(&db, &["replay", "compat-task"]);
    assert!(replay.contains("REPLAY OK"), "{replay}");
    assert!(replay.contains("true"), "{replay}");

    let snapshot = run(&db, &["snapshot", "compat-task"]);
    assert!(snapshot.contains("SNAPSHOT OK"), "{snapshot}");
    assert!(snapshot.contains("SNAPSHOT_BASE_GENERATION"), "{snapshot}");
    assert!(snapshot.contains("SNAPSHOT_GENERATION"), "{snapshot}");

    let restore = run(&db, &["restore", "compat-task"]);
    assert!(restore.contains("RESTORE OK"), "{restore}");
    assert!(restore.contains("SNAPSHOT_ID"), "{restore}");
    assert!(restore.contains("SNAPSHOT_GENERATION"), "{restore}");

    let replay_after = run(&db, &["replay", "compat-task"]);
    assert!(replay_after.contains("REPLAY OK"), "{replay_after}");
    assert!(replay_after.contains("true"), "{replay_after}");

    cleanup(&db);
}
