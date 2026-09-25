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

    let input = TaskInput::generic("implement database cache key mapping");

    let steps1 = Workflow::build_from_task_llm(&input).await.unwrap();
    let spec1 = deterministic_ai_kernel::workflow::contract::steps_to_exec_spec(&steps1);
    let plan_id_1 = spec1.spec_id.clone();

    let steps2 = Workflow::build_from_task_llm(&input).await.unwrap();
    let spec2 = deterministic_ai_kernel::workflow::contract::steps_to_exec_spec(&steps2);
    let plan_id_2 = spec2.spec_id.clone();

    assert_eq!(plan_id_1, plan_id_2);
    assert_eq!(steps1, steps2);
}
