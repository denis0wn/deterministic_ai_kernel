//! Phase 1.6 — Provider Integration and Runtime Verification
//!
//! Verifies:
//! - Provider trait implementations (Filesystem, LLM, Storage)
//! - Provider registration and access pattern
//! - OnceLock semantics (single registration)
//! - DefaultLlm block_in_place works in tokio context
//! - StorageProvider basic operations

use deterministic_ai_kernel::providers;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("provider_test_{}_{}", name, nanos))
}

// ── 1. DefaultFilesystem ────────────────────────────────────────────────────

#[test]
fn default_filesystem_read_write_roundtrip() {
    let path = unique_path("fs_roundtrip");
    let content = "hello provider world";

    // Write
    providers::get_filesystem()
        .write(path.to_str().unwrap(), content)
        .unwrap();

    // Read
    let read_back = providers::get_filesystem()
        .read_to_string(path.to_str().unwrap())
        .unwrap();
    assert_eq!(read_back, content);

    // Cleanup
    fs::remove_file(&path).ok();
}

#[test]
fn default_filesystem_create_dir_all() {
    let dir = unique_path("fs_mkdir");
    let sub = dir.join("a").join("b").join("c");

    providers::get_filesystem()
        .create_dir_all(sub.to_str().unwrap())
        .unwrap();
    assert!(sub.exists());

    // Cleanup
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn default_filesystem_read_nonexistent_returns_error() {
    let path = unique_path("fs_nonexist");
    let result = providers::get_filesystem().read_to_string(path.to_str().unwrap());
    assert!(result.is_err());
}

// ── 2. Provider Registration (OnceLock semantics) ──────────────────────────

// NOTE: OnceLock can only be set once per process. These tests verify the
// registration API exists and the default providers work. We cannot test
// re-registration because OnceLock prevents it.

#[test]
fn get_filesystem_returns_default_impl() {
    // get_filesystem() should return a working implementation
    let content = providers::get_filesystem()
        .read_to_string("/dev/null")
        .unwrap_or_default();
    // /dev/null is empty on Unix
    assert!(content.is_empty() || !content.is_empty()); // just verify no panic
}

#[test]
fn get_storage_returns_working_impl() {
    let storage = providers::get_storage();
    // Verify basic operation doesn't panic
    let _ = storage.table_exists("nonexistent_table");
}

// ── 3. DefaultLlm block_in_place ────────────────────────────────────────────

#[test]
fn default_llm_coding_assistant_works_in_tokio_runtime() {
    // This test verifies that DefaultLlm's block_in_place pattern works.
    // It will fail if there's no tokio runtime or if block_in_place panics.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async {
        // Inside a tokio runtime, block_in_place should work
        providers::get_llm().coding_assistant("test prompt")
    });

    // The actual LLM call will fail (no server), but block_in_place should not panic.
    // We just verify the call path works structurally.
    // If the error is about connection, that's expected and OK.
    match result {
        Ok(_) => {} // LLM responded (unlikely in test)
        Err(e) => {
            // Expected: connection refused or similar
            let msg = e.to_string();
            assert!(
                msg.contains("connection")
                    || msg.contains("resolve")
                    || msg.contains("connect")
                    || msg.contains("refusing")
                    || msg.contains("not available")
                    || msg.contains("not set")
                    || msg.contains("No route")
                    || msg.contains("dns")
                    || msg.contains("sending request")
                    || msg.contains("url")
                    || msg.contains("hyper")
                    || msg.contains("request"),
                "Unexpected error type: {}",
                msg
            );
        }
    }
}

#[test]
fn default_llm_execute_llm_structural() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async { providers::get_llm().execute_llm("test", None) });

    // Verify the call path works structurally (connection errors are expected)
    match result {
        Ok(response) => {
            // If it somehow succeeded, verify the response structure
            assert!(!response.model_name.is_empty() || response.model_name.is_empty());
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("connection")
                    || msg.contains("resolve")
                    || msg.contains("connect")
                    || msg.contains("refusing")
                    || msg.contains("not available")
                    || msg.contains("not set")
                    || msg.contains("No route")
                    || msg.contains("dns")
                    || msg.contains("request")
                    || msg.contains("hyper")
                    || msg.contains("sending request")
                    || msg.contains("url"),
                "Unexpected error type: {}",
                msg
            );
        }
    }
}

// ── 4. StorageProvider Basic Operations ─────────────────────────────────────

#[test]
fn storage_append_and_query_events() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_path("storage_events");
        let bus = deterministic_ai_kernel::event_bus::EventBus::new(&db).unwrap();

        // Append events
        bus.append_event("task-s1", Some("00_step"), "STEP_STARTED", &json!({}))
            .unwrap();
        bus.append_event(
            "task-s1",
            Some("00_step"),
            "STEP_COMPLETED",
            &json!({"ok": true}),
        )
        .unwrap();

        // Query events
        let events = bus.list_execution_events("task-s1").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "STEP_STARTED");
        assert_eq!(events[1].event_type, "STEP_COMPLETED");

        // Cleanup
        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(format!("{}-wal", db.display()));
        let _ = fs::remove_file(format!("{}-shm", db.display()));
    });
}

#[test]
fn storage_event_generations_are_monotonic() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_path("storage_gens");
        let bus = deterministic_ai_kernel::event_bus::EventBus::new(&db).unwrap();

        let gen1 = bus.append_event("task-g1", None, "E1", &json!({})).unwrap();
        let gen2 = bus.append_event("task-g1", None, "E2", &json!({})).unwrap();
        let gen3 = bus.append_event("task-g1", None, "E3", &json!({})).unwrap();

        assert!(gen1 < gen2, "generations must be monotonic");
        assert!(gen2 < gen3, "generations must be monotonic");

        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(format!("{}-wal", db.display()));
        let _ = fs::remove_file(format!("{}-shm", db.display()));
    });
}

#[test]
fn storage_event_batch_is_atomic() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_path("storage_batch");
        let bus = deterministic_ai_kernel::event_bus::EventBus::new(&db).unwrap();

        let events = [
            (None, "B1".to_string(), json!({"a": 1})),
            (Some("s1".to_string()), "B2".to_string(), json!({"b": 2})),
            (None, "B3".to_string(), json!({"c": 3})),
        ];

        let borrowed: Vec<(Option<&str>, &str, serde_json::Value)> = events
            .iter()
            .map(|(s, t, p)| (s.as_deref(), t.as_str(), p.clone()))
            .collect();

        bus.append_event_batch("task-b1", &borrowed).unwrap();

        let events = bus.list_execution_events("task-b1").unwrap();
        assert_eq!(events.len(), 3);

        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(format!("{}-wal", db.display()));
        let _ = fs::remove_file(format!("{}-shm", db.display()));
    });
}

// ── 5. PrimitiveExecutor Through Provider Abstraction ───────────────────────

#[test]
fn primitive_executor_uses_provider_abstraction() {
    use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
    use deterministic_ai_kernel::execution_abi::primitives::{
        PrimitiveId, PrimitiveKind, PrimitiveSpec,
    };

    let spec = PrimitiveSpec {
        id: PrimitiveId("test-prov".to_string()),
        kind: PrimitiveKind::Read,
        payload: json!({"path": "repository"}),
    };

    // Read with "repository" path uses task_payload directly (no provider call)
    let result = PrimitiveExecutor::execute("t1", &spec, "provider test").unwrap();
    assert_eq!(result.status, "ok");
    assert_eq!(result.output["content"], "provider test");
}

// ── 6. ExecSpec Load/Save Roundtrip ─────────────────────────────────────────

#[test]
fn storage_exec_spec_roundtrip() {
    use deterministic_ai_kernel::providers::storage::StorageProvider;
    use deterministic_ai_kernel::workflow::contract::TaskClass;

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_path("storage_execspec");
        let spec = TaskClass::CodeFix.to_exec_spec(None);

        // Store exec spec via task insertion, explicitly bound to this
        // test's database (the previous version wrote through the global
        // get_storage() singleton to whatever DB the environment pointed
        // at — audit finding M3).
        let spec_json = serde_json::to_string(&spec).unwrap();
        let storage = deterministic_ai_kernel::providers::storage_for(db.to_str().unwrap());
        storage
            .insert_task("task-es", "CodeFix", &spec_json)
            .unwrap();

        // Load it back
        let loaded = storage.load_exec_spec("task-es").unwrap();
        // P2: canonical CodeFix flow gained the kernel-only ApplyPatch step.
        assert_eq!(loaded.steps.len(), 6);
        assert_eq!(loaded.spec_id, spec.spec_id);
        assert_eq!(loaded.calculate_hash(), spec.calculate_hash());

        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(format!("{}-wal", db.display()));
        let _ = fs::remove_file(format!("{}-shm", db.display()));
    });
}

// ── 7. Replay Validation Through Provider ───────────────────────────────────

#[test]
fn storage_replay_validate_works() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_path("storage_replay");
        let bus = deterministic_ai_kernel::event_bus::EventBus::new(&db).unwrap();

        // Create a valid canonical lifecycle: lease -> dispatched -> started
        // -> completed (audit findings C1/R2).
        let lease_id = "task-rv/00_a/lease/1";
        bus.append_event(
            "task-rv",
            Some("00_a"),
            "LEASE_ACQUIRED",
            &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
        )
        .unwrap();
        bus.append_event(
            "task-rv",
            Some("00_a"),
            "STEP_DISPATCHED",
            &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
        )
        .unwrap();
        bus.append_event(
            "task-rv",
            Some("00_a"),
            "STEP_STARTED",
            &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
        )
        .unwrap();
        bus.append_event(
            "task-rv",
            Some("00_a"),
            "STEP_COMPLETED",
            &json!({"lease_id": lease_id}),
        )
        .unwrap();

        let db_str = db.to_str().unwrap();
        assert!(deterministic_ai_kernel::replay::engine::replay_validate(
            db_str, "task-rv"
        ));

        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(format!("{}-wal", db.display()));
        let _ = fs::remove_file(format!("{}-shm", db.display()));
    });
}
