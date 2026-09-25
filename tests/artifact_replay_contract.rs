//! Artifact replay contract.
//!
//! Verifies that repeated non-volatile compute results are stable and that
//! volatile operations are not stored as verified artifacts.

use anyhow::Result;
use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use deterministic_ai_kernel::providers;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dak_artifact_replay_{}_{}.db", label, nanos))
        .display()
        .to_string()
}

fn cleanup_db(db_path: &str) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path));
    let _ = std::fs::remove_file(format!("{}-shm", db_path));
}

fn make_compute_spec(op: &str) -> PrimitiveSpec {
    PrimitiveSpec {
        id: PrimitiveId("replay-test-prim".to_string()),
        kind: PrimitiveKind::Compute,
        payload: json!({ "operation": op }),
    }
}

#[test]
fn test_artifact_store_then_replay() -> Result<()> {
    let db_path = unique_db("replay");
    cleanup_db(&db_path);
    std::env::set_var("KERNEL_DB_PATH", &db_path);

    let bus = EventBus::new(&db_path)?;
    let task_id = "replay-task-001";

    let executor = PrimitiveExecutor::with_null_solver();
    let spec = make_compute_spec("echo deterministic_output");

    let first = executor.run(task_id, &spec, "")?;
    let second = executor.run(task_id, &spec, "")?;

    let events = providers::get_storage()
        .list_event_log(task_id)
        .unwrap_or_default();
    let has_store = events.iter().any(|(_, _, evt, _)| evt == "ARTIFACT_STORE");
    let has_replay = events
        .iter()
        .any(|(_, _, evt, _)| evt == "ARTIFACT_REPLAY_HIT");
    let has_cache_hit = events.iter().any(|(_, _, evt, _)| evt == "CACHE_HIT");

    assert!(
        has_store || has_replay || has_cache_hit,
        "expected replay-related events, got: {:?}",
        events
            .iter()
            .map(|(_, _, evt, _)| evt.clone())
            .collect::<Vec<_>>()
    );

    assert_eq!(first.output, second.output);
    assert_eq!(first.status, second.status);

    drop(bus);
    std::env::remove_var("KERNEL_DB_PATH");
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn test_volatile_op_not_stored() -> Result<()> {
    let db_path = unique_db("volatile");
    cleanup_db(&db_path);
    std::env::set_var("KERNEL_DB_PATH", &db_path);

    let bus = EventBus::new(&db_path)?;
    let task_id = "volatile-task-001";

    let executor = PrimitiveExecutor::with_null_solver();
    let spec = make_compute_spec("git_status");

    let _ = executor.run(task_id, &spec, "")?;

    let events = providers::get_storage()
        .list_event_log(task_id)
        .unwrap_or_default();
    let has_store = events.iter().any(|(_, _, evt, _)| evt == "ARTIFACT_STORE");
    assert!(!has_store, "volatile ops must NOT produce ARTIFACT_STORE");

    drop(bus);
    std::env::remove_var("KERNEL_DB_PATH");
    cleanup_db(&db_path);
    Ok(())
}
