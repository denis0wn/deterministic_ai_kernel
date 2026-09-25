use std::sync::{Mutex, OnceLock};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use serde_json::json;

#[tokio::test]
async fn test_primitive_read_equivalence() {
    let _guard = test_lock();

    let file_path = "temp_cacheable_file.txt";
    std::fs::write(file_path, "original content").unwrap();

    let spec = PrimitiveSpec {
        id: PrimitiveId("read-1".to_string()),
        kind: PrimitiveKind::Read,
        payload: json!({ "path": file_path }),
    };

    let res1 = PrimitiveExecutor::execute("task-1", &spec, "").unwrap();
    let res2 = PrimitiveExecutor::execute("task-1", &spec, "").unwrap();

    assert_eq!(res1.status, "ok");
    assert_eq!(res2.status, "ok");
    assert_eq!(res1.output, res2.output);

    let _ = std::fs::remove_file(file_path);
}

#[tokio::test]
async fn test_primitive_read_dependency_change_reflected() {
    let _guard = test_lock();

    let file_path = "temp_cacheable_file_invalidation.txt";
    std::fs::write(file_path, "original content").unwrap();

    let spec = PrimitiveSpec {
        id: PrimitiveId("read-2".to_string()),
        kind: PrimitiveKind::Read,
        payload: json!({ "path": file_path }),
    };

    let res1 = PrimitiveExecutor::execute("task-2", &spec, "").unwrap();
    assert_eq!(res1.status, "ok");

    std::fs::write(file_path, "modified content").unwrap();

    let res2 = PrimitiveExecutor::execute("task-2", &spec, "").unwrap();
    assert_eq!(res2.status, "ok");
    assert_ne!(res1.output, res2.output);

    let _ = std::fs::remove_file(file_path);
}
