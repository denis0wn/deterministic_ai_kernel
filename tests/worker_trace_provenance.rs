use deterministic_ai_kernel::event_bus::EventBus;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "deterministic_ai_kernel_{}_{}.db",
        test_name, nanos
    ))
}

fn bin_path() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_deterministic_ai_kernel") {
        return PathBuf::from(p);
    }
    let mut p = std::env::current_exe().expect("test failure");
    p.pop();
    p.pop();
    p.push("deterministic_ai_kernel");
    p
}

fn run(db: &PathBuf, args: &[&str]) -> String {
    let out = Command::new(bin_path())
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .expect("test failure");
    assert!(
        out.status.success(),
        "command failed: {:?}\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn setup_db(db: &PathBuf) {
    let _ = fs::remove_file(db);
    let bus = EventBus::new(db).expect("test failure");
    drop(bus);

    let conn = Connection::open(db).expect("test failure");

    conn.execute(
        "INSERT INTO tasks (task_id, task_class) VALUES (?1, ?2)",
        ("task-trace", "Generic"),
    )
    .expect("test failure");

    conn.execute(
        "INSERT INTO step_status (task_id, step_id, status) VALUES (?1, ?2, 'dispatched')",
        ("task-trace", "00_analyze_task"),
    )
    .expect("test failure");

    conn.execute(
        "INSERT INTO leases (lease_id, task_id, step_id, worker_id, state, acquired_generation, expires_at_generation)
         VALUES (?1, ?2, ?3, 'worker-scheduler', 'active', 1, 100)",
        ("lease-trace", "task-trace", "00_analyze_task"),
    )
    .expect("test failure");
}

#[test]
fn worker_lifecycle_emits_traceable_event_sequence() {
    let db = unique_db_path("worker_trace_provenance");
    setup_db(&db);

    run(&db, &["claim-worker", "task-trace", "worker-A"]);
    run(
        &db,
        &["start-step", "task-trace", "worker-A", "00_analyze_task"],
    );
    run(
        &db,
        &["heartbeat", "task-trace", "worker-A", "00_analyze_task"],
    );
    run(
        &db,
        &["complete-step", "task-trace", "worker-A", "00_analyze_task"],
    );

    let bus = EventBus::new(&db).expect("test failure");
    let events = bus
        .list_execution_events("task-trace")
        .expect("test failure");
    let kinds: Vec<_> = events.iter().map(|e| e.event_type.as_str()).collect();

    assert!(kinds.len() >= 4, "unexpected event sequence: {:?}", kinds);
    assert_eq!(kinds[0], "WORKER_CLAIMED");
    assert_eq!(kinds[1], "STEP_STARTED");
    assert_eq!(kinds[2], "WORKER_HEARTBEAT");
    assert_eq!(kinds[3], "STEP_COMPLETED");
    if kinds.len() > 4 {
        assert_eq!(&kinds[4..], &["LEASE_ACQUIRED", "STEP_DISPATCHED"]);
    }

    let graph = bus.build_state_graph("task-trace").expect("test failure");
    assert!(graph.nodes.len() >= 4);

    let _ = fs::remove_file(&db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}
