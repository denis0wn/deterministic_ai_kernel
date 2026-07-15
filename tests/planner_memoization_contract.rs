struct MockBackendGuard {
    prev: Option<String>,
}

impl MockBackendGuard {
    fn install() -> Self {
        let prev = std::env::var("DAK_LM_BACKEND").ok();
        std::env::set_var("DAK_LM_BACKEND", "mock");
        Self { prev }
    }
}

impl Drop for MockBackendGuard {
    fn drop(&mut self) {
        match self.prev.take() {
            Some(value) => std::env::set_var("DAK_LM_BACKEND", value),
            None => std::env::remove_var("DAK_LM_BACKEND"),
        }
    }
}

use deterministic_ai_kernel::workflow::compiler::{TaskInput, Workflow};

#[tokio::test]
async fn test_planner_memoization_hit() {
    let _mock_backend = MockBackendGuard::install();
    let db_path = "planner_cache_hit.db";
    let _ = std::fs::remove_file(db_path);

    // Override the DB path
    deterministic_ai_kernel::providers::get_storage().set_override_path(Some(db_path.to_string()));

    let input = TaskInput::generic("implement database cache key mapping");

    // First run (Cache Miss)
    let steps1 = Workflow::build_from_task_llm(&input).await.unwrap();
    let spec1 = deterministic_ai_kernel::workflow::contract::steps_to_exec_spec(&steps1);
    let plan_id_1 = spec1.spec_id.clone();

    // Check that we got a PLANNER_CACHE_MISS event logged
    let events = deterministic_ai_kernel::providers::get_storage()
        .query_events("global_task")
        .unwrap();
    let miss_exists = events.iter().any(|e| e.event_type == "PLANNER_CACHE_MISS");
    assert!(
        miss_exists,
        "PLANNER_CACHE_MISS event was not logged on first run!"
    );

    // Second run (Cache Hit)
    let steps2 = Workflow::build_from_task_llm(&input).await.unwrap();
    let spec2 = deterministic_ai_kernel::workflow::contract::steps_to_exec_spec(&steps2);
    let plan_id_2 = spec2.spec_id.clone();

    // Check that PLANNER_CACHE_HIT was logged
    let events_after = deterministic_ai_kernel::providers::get_storage()
        .query_events("global_task")
        .unwrap();
    let hit_exists = events_after
        .iter()
        .any(|e| e.event_type == "PLANNER_CACHE_HIT");
    assert!(
        hit_exists,
        "PLANNER_CACHE_HIT event was not logged on second run!"
    );

    // Assert returns exact same plan_id (HIT)
    assert_eq!(plan_id_1, plan_id_2);
    assert_eq!(steps1, steps2);

    // Cleanup
    let _ = std::fs::remove_file(db_path);
    deterministic_ai_kernel::providers::get_storage().set_override_path(None);
}
