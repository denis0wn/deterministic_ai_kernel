//! PROGRESS UNTIL VERIFIED — stage 2.
//!
//! The REPETITION observation event (kernel-owned progress ledger) must
//! be fold-compatible: a canonical lifecycle plus a standalone REPETITION
//! unit must pass replay_validate, and the observation must not mutate
//! step/task state.

use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::providers::storage::StorageProvider;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("progress_fold_{}_{}.db", test_name, nanos))
}

fn cleanup(db: &PathBuf) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}

fn emit_step_lifecycle(bus: &EventBus, task: &str, step: &str) {
    let lease_id = format!("{task}/{step}/lease/1");
    bus.append_event(
        task,
        Some(step),
        "LEASE_ACQUIRED",
        &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
    )
    .unwrap();
    bus.append_event(
        task,
        Some(step),
        "STEP_DISPATCHED",
        &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
    )
    .unwrap();
    bus.append_event(
        task,
        Some(step),
        "STEP_STARTED",
        &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
    )
    .unwrap();
    bus.append_event(
        task,
        Some(step),
        "STEP_COMPLETED",
        &json!({"lease_id": lease_id, "outcome": "ok"}),
    )
    .unwrap();
}

#[test]
fn repetition_observation_is_fold_compatible() {
    let db = unique_db("repetition_fold");
    let bus = EventBus::new(&db).unwrap();

    emit_step_lifecycle(&bus, "task-p", "00_step");

    // Standalone observation unit, as emitted by pipeline-run stage 2.
    bus.append_event(
        "task-p",
        None,
        "REPETITION",
        &json!({
            "task_id": "task-p",
            "repeats_task": "task-0",
            "payload_fingerprint": "deadbeef",
            "failure_signature": "fatal: example",
            "policy": "progress_until_verified/stage2_detect_only",
        }),
    )
    .unwrap();

    let db_str = db.to_str().unwrap();
    assert!(
        deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-p"),
        "replay_validate must accept a canonical lifecycle plus a REPETITION observation"
    );
    let storage = deterministic_ai_kernel::providers::storage_for(db_str);
    assert!(
        storage
            .replay_violations("task-p")
            .unwrap_or_default()
            .is_empty(),
        "REPETITION observation must not produce fold violations"
    );
    cleanup(&db);
}

#[test]
fn repetition_observation_does_not_change_task_state() {
    let db = unique_db("repetition_state");
    let bus = EventBus::new(&db).unwrap();

    emit_step_lifecycle(&bus, "task-q", "00_step");
    let storage = deterministic_ai_kernel::providers::storage_for(db.to_str().unwrap());
    let before = storage.task_state("task-q").unwrap();

    bus.append_event(
        "task-q",
        None,
        "REPETITION",
        &json!({"task_id": "task-q", "repeats_task": "task-0"}),
    )
    .unwrap();

    let after = deterministic_ai_kernel::providers::storage_for(db.to_str().unwrap())
        .task_state("task-q")
        .unwrap();
    assert_eq!(
        format!("{before:?}"),
        format!("{after:?}"),
        "REPETITION is observation-only and must not change task state"
    );
    cleanup(&db);
}

/// Stage 3: TASK_TERMINAL_ASSESSED (terminal taxonomy observation) is
/// fold-compatible and state-neutral, like REPETITION.
#[test]
fn terminal_assessment_observation_is_fold_compatible() {
    let db = unique_db("terminal_fold");
    let bus = EventBus::new(&db).unwrap();

    emit_step_lifecycle(&bus, "task-r", "00_step");

    bus.append_event(
        "task-r",
        None,
        "TASK_TERMINAL_ASSESSED",
        &json!({
            "task_id": "task-r",
            "payload_fingerprint": "abcd",
            "taxonomy": "VerifiedSuccess",
            "basis": "completed under kernel-owned verification",
            "policy": "progress_until_verified/stage3",
        }),
    )
    .unwrap();

    let db_str = db.to_str().unwrap();
    assert!(
        deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-r"),
        "replay_validate must accept a canonical lifecycle plus TASK_TERMINAL_ASSESSED"
    );
    let storage = deterministic_ai_kernel::providers::storage_for(db_str);
    assert!(
        storage
            .replay_violations("task-r")
            .unwrap_or_default()
            .is_empty(),
        "TASK_TERMINAL_ASSESSED must not produce fold violations"
    );
    cleanup(&db);
}

/// Stage 4: decomposition records (SUBTASK_OF on the lemma,
/// TASK_DECOMPOSED on the carrier) are fold-compatible and
/// state-neutral.
#[test]
fn decomposition_records_are_fold_compatible() {
    let db = unique_db("decomposition_fold");
    let bus = EventBus::new(&db).unwrap();

    emit_step_lifecycle(&bus, "task-lemma", "00_step");
    emit_step_lifecycle(&bus, "task-carrier", "00_step");

    bus.append_event(
        "task-lemma",
        None,
        "SUBTASK_OF",
        &json!({
            "task_id": "task-lemma",
            "carrier_label": "task-carrier",
            "role": "lemma",
            "policy": "progress_until_verified/stage4",
        }),
    )
    .unwrap();
    bus.append_event(
        "task-carrier",
        None,
        "TASK_DECOMPOSED",
        &json!({
            "task_id": "task-carrier",
            "subtasks": ["task-lemma"],
            "composition_contract": {
                "target_file": "t.py",
                "regions": "disjoint",
                "acceptance": "carrier task-level tests",
            },
            "policy": "progress_until_verified/stage4",
        }),
    )
    .unwrap();

    let db_str = db.to_str().unwrap();
    assert!(
        deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-lemma"),
        "replay_validate must accept SUBTASK_OF"
    );
    assert!(
        deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-carrier"),
        "replay_validate must accept TASK_DECOMPOSED"
    );
    let storage = deterministic_ai_kernel::providers::storage_for(db_str);
    assert!(
        storage
            .replay_violations("task-lemma")
            .unwrap_or_default()
            .is_empty(),
        "SUBTASK_OF must not produce fold violations"
    );
    assert!(
        storage
            .replay_violations("task-carrier")
            .unwrap_or_default()
            .is_empty(),
        "TASK_DECOMPOSED must not produce fold violations"
    );
    cleanup(&db);
}
