use std::sync::{Mutex, OnceLock};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use serde_json::json;

#[tokio::test]
async fn test_primitive_cache_hit_equivalence() {
    let _guard = test_lock();
    let db_path = "primitive_cache_hit.db";
    let _ = std::fs::remove_file(db_path);

    // Override the DB path
    deterministic_ai_kernel::providers::get_storage().set_override_path(Some(db_path.to_string()));

    // Create a temporary file to read
    let file_path = "temp_cacheable_file.txt";
    std::fs::write(file_path, "original content").unwrap();

    let spec = PrimitiveSpec {
        id: PrimitiveId("read-1".to_string()),
        kind: PrimitiveKind::Read,
        payload: json!({ "path": file_path }),
    };

    // First execution (Cache Miss)
    let res1 = PrimitiveExecutor::execute("task-1", &spec, "").unwrap();
    assert_eq!(res1.status, "ok");

    // Second execution (Cache Hit)
    let res2 = PrimitiveExecutor::execute("task-1", &spec, "").unwrap();
    assert_eq!(res2.status, "ok");

    // Let's check that CACHE_HIT event was logged
    let events = deterministic_ai_kernel::providers::get_storage()
        .query_events("task-1")
        .unwrap();
    let hit_exists = events.iter().any(|e| e.event_type == "CACHE_HIT");
    assert!(hit_exists, "CACHE_HIT event was not logged!");

    // Cleanup
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(file_path);
    deterministic_ai_kernel::providers::get_storage().set_override_path(None);
}

#[tokio::test]
async fn test_primitive_cache_dependency_invalidation() {
    let _guard = test_lock();
    let db_path = "primitive_cache_invalidation.db";
    let _ = std::fs::remove_file(db_path);

    // Override the DB path
    deterministic_ai_kernel::providers::get_storage().set_override_path(Some(db_path.to_string()));

    // Create a temporary file to read
    let file_path = "temp_cacheable_file_invalidation.txt";
    std::fs::write(file_path, "original content").unwrap();

    let spec = PrimitiveSpec {
        id: PrimitiveId("read-2".to_string()),
        kind: PrimitiveKind::Read,
        payload: json!({ "path": file_path }),
    };

    // First execution (Cache Miss)
    let res1 = PrimitiveExecutor::execute("task-2", &spec, "").unwrap();
    assert_eq!(res1.status, "ok");

    // Modify the file contents to trigger invalidation (different dependency_hash)
    std::fs::write(file_path, "modified content").unwrap();

    // Second execution (Cache Miss again because dependency_hash changed)
    let res2 = PrimitiveExecutor::execute("task-2", &spec, "").unwrap();
    assert_eq!(res2.status, "ok");

    // Let's check events: we should have CACHE_MISS logged twice and no CACHE_HIT
    let events = deterministic_ai_kernel::providers::get_storage()
        .query_events("task-2")
        .unwrap();
    let misses = events
        .iter()
        .filter(|e| e.event_type == "CACHE_MISS")
        .count();
    let hits = events
        .iter()
        .filter(|e| e.event_type == "CACHE_HIT")
        .count();

    assert_eq!(misses, 2, "Expected 2 CACHE_MISS events, got {}", misses);
    assert_eq!(hits, 0, "Expected 0 CACHE_HIT events, got {}", hits);

    // Cleanup
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(file_path);
    deterministic_ai_kernel::providers::get_storage().set_override_path(None);
}
