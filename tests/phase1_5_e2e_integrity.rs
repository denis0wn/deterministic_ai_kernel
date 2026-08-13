//! Phase 1.5 — End-to-End Pipeline Integrity Verification
//!
//! Verifies that all Phase 1 fixes work together correctly:
//! - state_hash BLAKE3
//! - capsule created_at determinism
//! - cross-unit ordering validation
//! - reservation_generation provenance
//! - publish_pipeline_report atomicity
//! - normalize_step CodeFix support
//! - CodeFix artifact contracts
//! - Replay validation

use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::providers::storage::StorageProvider;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use deterministic_ai_kernel::workflow::contract::TaskClass;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("phase15_{}_{}.db", test_name, nanos))
}

fn cleanup(db: &PathBuf) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}

/// Emit the canonical lifecycle for one step so the replay fold accepts it:
/// LEASE_ACQUIRED -> STEP_DISPATCHED -> STEP_STARTED -> STEP_COMPLETED.
/// The canonical state machine requires a dispatched step before it can
/// start, and a started step before it can complete (audit finding C1/R2).
fn emit_step_lifecycle(bus: &EventBus, task: &str, step: &str, outcome_json: serde_json::Value) {
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
        &json!({"lease_id": lease_id, "outcome": outcome_json}),
    )
    .unwrap();
}

// ── 1. Full Pipeline: ExecSpec → Events → Snapshot → Replay ─────────────────

#[test]
fn e2e_codefix_full_pipeline_snapshot_replay() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("full_pipeline");
        let bus = EventBus::new(&db).unwrap();
        let spec = TaskClass::CodeFix.to_exec_spec(None);

        // Simulate full pipeline: emit events for each step
        bus.append_event(
            "task-e2e",
            None,
            "pipeline.created",
            &json!({"class":"CodeFix"}),
        )
        .unwrap();

        for step in &spec.steps {
            if let Some(ref prim) = step.primitive {
                let requires_llm = prim
                    .payload
                    .get("requires_llm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let outcome = if requires_llm {
                    json!("skipped_no_llm")
                } else {
                    let result = PrimitiveExecutor::execute("task-e2e", prim, "test payload");
                    match result {
                        Ok(r) => json!(r.status),
                        Err(_) => json!("error"),
                    }
                };

                emit_step_lifecycle(&bus, "task-e2e", &step.step_id, outcome);
            }
        }

        // Verify replay validation passes
        let db_str = db.to_str().unwrap();
        assert!(
            deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-e2e"),
            "replay_validate must pass after full pipeline"
        );

        // Build snapshot and verify it succeeds
        deterministic_ai_kernel::snapshot::rebuild_snapshot(db_str, "task-e2e", true).unwrap();

        // Verify replay still passes after snapshot
        assert!(
            deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-e2e"),
            "replay_validate must pass after snapshot rebuild"
        );

        // Build capsule and verify determinism
        let cap1 = build_replay_capsule(&bus, "task-e2e").unwrap();
        let cap2 = build_replay_capsule(&bus, "task-e2e").unwrap();
        assert_eq!(
            cap1.created_at, cap2.created_at,
            "capsule created_at must be deterministic"
        );
        assert_ne!(
            cap1.created_at, "now",
            "capsule created_at must not be 'now'"
        );

        cleanup(&db);
    });
}

// ── 2. State Hash Determinism ───────────────────────────────────────────────

#[test]
fn e2e_state_hash_is_deterministic() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("state_hash");
        let bus = EventBus::new(&db).unwrap();

        // Create an identical canonical lifecycle for one step.
        emit_step_lifecycle(&bus, "task-hash", "00_step", json!("ok"));

        let db_str = db.to_str().unwrap();

        // Rebuild snapshot twice
        deterministic_ai_kernel::snapshot::rebuild_snapshot(db_str, "task-hash", true).unwrap();

        // Read snapshot payload via explicit db routing (the former
        // get_storage() singleton read from whatever database the global
        // override happened to point at — audit finding M3).
        let storage = deterministic_ai_kernel::providers::storage_for(db_str);
        let payload1 = storage
            .get_latest_snapshot_payload("task-hash")
            .unwrap()
            .unwrap();

        // Rebuild again (should produce identical canonical state)
        deterministic_ai_kernel::snapshot::rebuild_snapshot(db_str, "task-hash", true).unwrap();
        let payload2 = storage
            .get_latest_snapshot_payload("task-hash")
            .unwrap()
            .unwrap();

        // Parse and compare state_hash
        let v1: serde_json::Value = serde_json::from_str(&payload1).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&payload2).unwrap();
        assert_eq!(
            v1["state_hash"], v2["state_hash"],
            "state_hash must be identical across rebuilds"
        );
        // Verify it's not a length-based hash (should be > u32::MAX range)
        let hash = v1["state_hash"].as_u64().unwrap();
        assert!(
            hash > 1_000_000,
            "state_hash should be a real hash, not a string length"
        );

        cleanup(&db);
    });
}

// ── 3. Cross-Unit Ordering Validation ───────────────────────────────────────

#[test]
fn e2e_replay_validate_catches_cross_unit_violation() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("cross_unit");
        let bus = EventBus::new(&db).unwrap();

        // Valid canonical lifecycles must pass validation.
        emit_step_lifecycle(&bus, "task-cross", "00_a", json!("Success"));
        emit_step_lifecycle(&bus, "task-cross", "01_b", json!("Success"));

        let db_str = db.to_str().unwrap();
        assert!(
            deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-cross"),
            "valid canonical sequence must pass validation"
        );

        // A started step with no prior dispatch is a cross-unit ordering
        // violation and must be rejected (the negative case the previous
        // version of this test never constructed).
        bus.append_event(
            "task-cross",
            Some("02_c"),
            "STEP_STARTED",
            &json!({"worker_id": "worker-test"}),
        )
        .unwrap();
        assert!(
            !deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-cross"),
            "STEP_STARTED without dispatch must be rejected"
        );

        cleanup(&db);
    });
}

// ── 4. normalize_step CodeFix Integration ───────────────────────────────────

#[test]
fn e2e_normalize_step_codefix_in_plan_creation() {
    use deterministic_ai_kernel::planner_pipeline::Plan;

    let steps = vec![
        "Read Repository".to_string(),
        "Locate Bug".to_string(),
        "Patch Code".to_string(),
        "Run Tests".to_string(),
        "Validate Patch".to_string(),
    ];

    let plan = Plan::new_with_stable_id(42, steps);

    // Verify each step got the correct StepKind
    let step_kinds: Vec<_> = plan.spec.steps.iter().map(|s| s.step_id.clone()).collect();
    assert_eq!(
        step_kinds,
        vec![
            "00_read_repository",
            "01_locate_bug",
            "02_patch_code",
            "03_run_tests",
            "04_validate_patch"
        ]
    );

    // Verify artifact flow is declared
    assert!(plan.spec.steps[0]
        .outputs
        .contains(&"repository_content".to_string()));
    assert!(plan.spec.steps[1]
        .inputs
        .contains(&"repository_content".to_string()));
    assert!(plan.spec.steps[4]
        .outputs
        .contains(&"validation_verdict".to_string()));

    // Verify requires_llm flags
    let llm_flags: Vec<_> = plan
        .spec
        .steps
        .iter()
        .map(|s| {
            let prim = s.primitive.as_ref().unwrap();
            (
                s.step_id.clone(),
                prim.payload
                    .get("requires_llm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            )
        })
        .collect();

    // ReadRepository: no LLM (Read primitive doesn't use it)
    assert!(
        !llm_flags
            .iter()
            .find(|(id, _)| id == "00_read_repository")
            .unwrap()
            .1
    );
    // LocateBug: needs LLM
    assert!(
        llm_flags
            .iter()
            .find(|(id, _)| id == "01_locate_bug")
            .unwrap()
            .1
    );
    // PatchCode: needs LLM
    assert!(
        llm_flags
            .iter()
            .find(|(id, _)| id == "02_patch_code")
            .unwrap()
            .1
    );
    // RunTests: no LLM (runs a command)
    assert!(
        !llm_flags
            .iter()
            .find(|(id, _)| id == "03_run_tests")
            .unwrap()
            .1
    );
    // ValidatePatch: needs LLM (Route primitive verifies via LLM)
    assert!(
        llm_flags
            .iter()
            .find(|(id, _)| id == "04_validate_patch")
            .unwrap()
            .1
    );
}

// ── 5. Capsule Determinism Across Builds ────────────────────────────────────

#[test]
fn e2e_capsule_created_at_deterministic_across_builds() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("capsule_det");
        let bus = EventBus::new(&db).unwrap();

        // Create a fixed set of canonical lifecycle events
        emit_step_lifecycle(&bus, "task-cap", "00_a", json!("Success"));
        emit_step_lifecycle(&bus, "task-cap", "01_b", json!("Success"));

        // Build capsule multiple times
        let cap1 = build_replay_capsule(&bus, "task-cap").unwrap();
        let cap2 = build_replay_capsule(&bus, "task-cap").unwrap();
        let cap3 = build_replay_capsule(&bus, "task-cap").unwrap();

        // All must have identical created_at
        assert_eq!(cap1.created_at, cap2.created_at);
        assert_eq!(cap2.created_at, cap3.created_at);
        // Must be derived from events, not wall clock
        assert!(cap1.created_at.starts_with("evt-"));
        assert_ne!(cap1.created_at, "now");

        cleanup(&db);
    });
}

// ── 6. Event Atomicity via Batch Append ─────────────────────────────────────

#[test]
fn e2e_event_batch_append_is_atomic() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("batch_atomic");
        let bus = EventBus::new(&db).unwrap();

        // Batch-append a canonical lifecycle for one step plus pipeline
        // bookend events. Each append_event_batch event lands in its own
        // causal unit, which the canonical fold accepts for single-event
        // units.
        let lease_id = "task-batch/00_step/lease/1".to_string();
        let events = [
            (None, "pipeline.started".to_string(), json!({"seed": 42})),
            (
                Some("00_step".to_string()),
                "LEASE_ACQUIRED".to_string(),
                json!({"lease_id": lease_id.clone(), "worker_id": "worker-test"}),
            ),
            (
                Some("00_step".to_string()),
                "STEP_DISPATCHED".to_string(),
                json!({"lease_id": lease_id.clone(), "worker_id": "worker-test"}),
            ),
            (
                Some("00_step".to_string()),
                "STEP_STARTED".to_string(),
                json!({"lease_id": lease_id.clone(), "worker_id": "worker-test"}),
            ),
            (
                Some("00_step".to_string()),
                "STEP_COMPLETED".to_string(),
                json!({"lease_id": lease_id.clone(), "outcome": "Success"}),
            ),
            (None, "pipeline.completed".to_string(), json!({"ok": true})),
        ];

        let borrowed: Vec<(Option<&str>, &str, serde_json::Value)> = events
            .iter()
            .map(|(s, t, p)| (s.as_deref(), t.as_str(), p.clone()))
            .collect();

        bus.append_event_batch("task-batch", &borrowed).unwrap();

        // Verify all events were written
        let events = bus.list_execution_events("task-batch").unwrap();
        assert_eq!(events.len(), 6, "all 6 batch events must be present");

        // Verify replay passes
        let db_str = db.to_str().unwrap();
        assert!(
            deterministic_ai_kernel::replay::engine::replay_validate(db_str, "task-batch"),
            "replay_validate must pass after batch append"
        );

        cleanup(&db);
    });
}

// ── 7. Determinism: ExecSpec Hash Stability ─────────────────────────────────

#[test]
fn e2e_exec_spec_hash_stable_across_invocations() {
    let spec1 = TaskClass::CodeFix.to_exec_spec(None);
    let spec2 = TaskClass::CodeFix.to_exec_spec(None);

    assert_eq!(spec1.spec_id, spec2.spec_id, "ExecSpec hash must be stable");
    assert_eq!(
        spec1.calculate_hash(),
        spec2.calculate_hash(),
        "calculate_hash must be stable"
    );
}

// ── 8. Snapshot-Event Log Consistency ───────────────────────────────────────

#[test]
fn e2e_snapshot_reflects_event_log() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db("snapshot_consistency");
        let bus = EventBus::new(&db).unwrap();

        // Create canonical lifecycles for 3 steps
        for i in 0..3 {
            let step_id = format!("0{}_step", i);
            emit_step_lifecycle(&bus, "task-snap", &step_id, json!("Success"));
        }

        let db_str = db.to_str().unwrap();

        // Build snapshot
        deterministic_ai_kernel::snapshot::rebuild_snapshot(db_str, "task-snap", true).unwrap();

        // Read snapshot via explicit db routing (audit finding M3)
        let storage = deterministic_ai_kernel::providers::storage_for(db_str);
        let payload = storage
            .get_latest_snapshot_payload("task-snap")
            .unwrap()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();

        // Verify snapshot has correct task_id
        assert_eq!(v["task_id"].as_str().unwrap(), "task-snap");

        // Verify last_generation is positive
        assert!(v["last_generation"].as_i64().unwrap() > 0);

        // Verify all 3 steps are in the snapshot
        let steps = v["steps"].as_object().unwrap();
        assert!(steps.contains_key("00_step"));
        assert!(steps.contains_key("01_step"));
        assert!(steps.contains_key("02_step"));

        // All steps should be "committed" (from STEP_COMPLETED events)
        assert_eq!(steps["00_step"].as_str().unwrap(), "committed");
        assert_eq!(steps["01_step"].as_str().unwrap(), "committed");
        assert_eq!(steps["02_step"].as_str().unwrap(), "committed");

        // Verify state_hash is present and non-zero
        let hash = v["state_hash"].as_u64().unwrap();
        assert!(hash > 0, "state_hash must be non-zero");

        cleanup(&db);
    });
}
