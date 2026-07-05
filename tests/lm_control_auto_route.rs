mod test_util;

use deterministic_ai_kernel::lm_control::auto_route;
use serial_test::serial;
use test_util::with_mock_lm_backend;

/// Uses mock LM backend and should run in the default test suite.
#[test]
#[serial]
fn auto_route_succeeds_for_task_planning_when_ready() {
    with_mock_lm_backend(|| {
        auto_route("task_planning").unwrap();
    });
}

#[test]
#[serial]
fn send_prompt_returns_mock_response_for_task_planning() {
    with_mock_lm_backend(|| {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(deterministic_ai_kernel::lm_control::send_prompt(
                "task_planning",
                "list steps to deploy a Rust binary",
            ))
            .unwrap();
        assert!(result.contains("mock"));
    });
}
