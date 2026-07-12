use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
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
        .expect("test failure");

    assert!(
        out.status.success(),
        "command failed: {:?}\nstderr=\n{}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );

    String::from_utf8(out.stdout).expect("test failure")
}

#[test]
fn snapshot_restore_contract_is_backward_compatible_with_empty_or_uninitialized_db() {
    let db = unique_db("versioned_snapshot_compatibility");
    cleanup(&db);

    let snapshot = run(&db, &["snapshot", "versioned-task"]);
    assert!(snapshot.contains("SNAPSHOT OK"), "{snapshot}");

    let restore = run(&db, &["restore", "versioned-task"]);
    assert!(restore.contains("RESTORE OK"), "{restore}");

    cleanup(&db);
}
