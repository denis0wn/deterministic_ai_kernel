mod effects;
mod event_bus;
mod execution;
mod leases;
mod replay;
mod scheduler;
mod snapshot;
mod worker;
mod workflow;

use effects::execute_effects;
use event_bus::EventBus;
use execution::engine::ExecutionEngine;
use leases::{expire_leases, seed_demo_leases};
use replay::engine::replay_validate;
use rusqlite::Connection;
use scheduler::{reconcile, schedule};
use snapshot::{rebuild_snapshot, restore_snapshot};
use std::fs;

fn print_stats(db: &str) {
    let conn = Connection::open(db).unwrap();

    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0))
        .unwrap_or(0);

    let causal_units: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT causal_unit_id) FROM event_log",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let max_generation: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let tasks: i64 = conn
        .query_row("SELECT COUNT(DISTINCT task_id) FROM event_log", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);

    println!("EVENTS: {}", events);
    println!("CAUSAL_UNITS: {}", causal_units);
    println!("MAX_GENERATION: {}", max_generation);
    println!("TASKS: {}", tasks);
}

fn reset_db(db: &str) {
    if !std::path::Path::new(db).exists() {
        println!("RESET OK");
        return;
    }

    let conn = Connection::open(db).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA wal_checkpoint(FULL);
        DELETE FROM event_log;
        DELETE FROM effect_ledger;
        DELETE FROM step_dependencies;
        DELETE FROM step_status;
        DELETE FROM state_snapshots;
        DELETE FROM generations;
        VACUUM;
        "#,
    )
    .unwrap();

    println!("RESET OK");
}

fn vacuum_db(db: &str) {
    if !std::path::Path::new(db).exists() {
        println!("VACUUM OK");
        return;
    }

    let conn = Connection::open(db).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA wal_checkpoint(FULL);
        VACUUM;
        "#,
    )
    .unwrap();

    println!("VACUUM OK");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let db = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| "kernel.db".to_string());
    let db = db.as_str();

    match args.get(1).map(|s| s.as_str()) {
        Some("reset") => {
            reset_db(db);
            return;
        }
        Some("replay") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let ok = replay_validate(db, task_id);
            println!("REPLAY OK: {}", ok);
            return;
        }
        Some("stats") => {
            print_stats(db);
            return;
        }
        Some("vacuum") => {
            vacuum_db(db);
            return;
        }
        Some("snapshot") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            rebuild_snapshot(db, task_id).unwrap();
            return;
        }
        Some("restore") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            restore_snapshot(db, task_id).unwrap();
            return;
        }
        Some("schedule") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            schedule(db, task_id).unwrap();
            return;
        }
        Some("reconcile") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            reconcile(db, task_id).unwrap();
            return;
        }
        Some("execute-effects") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            execute_effects(db, task_id).unwrap();
            return;
        }
        Some("seed-leases") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            seed_demo_leases(db, task_id).unwrap();
            return;
        }
        Some("expire-leases") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            expire_leases(db, task_id).unwrap();
            return;
        }
        Some("claim-worker") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            worker::claim_worker(db, task_id, worker_id).unwrap();
            return;
        }
        Some("complete-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            worker::complete_step(db, task_id, worker_id, step_id).unwrap();
            return;
        }
        Some("fail-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            let reason = args.get(5).map(|s| s.as_str()).unwrap_or("worker_error");
            worker::fail_step(db, task_id, worker_id, step_id, reason).unwrap();
            return;
        }
        Some("start-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            worker::start_step(db, task_id, worker_id, step_id).unwrap();
            return;
        }
        Some("heartbeat") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            worker::heartbeat(db, task_id, worker_id, step_id).unwrap();
            return;
        }
        Some("rmdb") => {
            let _ = fs::remove_file(db);
            println!("RMDB OK");
            return;
        }
        _ => {}
    }

    let bus = EventBus::new(db).unwrap();
    let engine = ExecutionEngine::new(bus.clone());
    engine.run("task1");

    let events = bus.query("task1").unwrap();
    println!("EVENTS: {}", events.len());

    let ok = replay_validate(db, "task1");
    println!("REPLAY OK: {}", ok);
}
