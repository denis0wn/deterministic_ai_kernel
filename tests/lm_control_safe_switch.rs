mod test_util;

use deterministic_ai_kernel::api;
use test_util::with_mock_lm_backend;

#[test]
fn safe_switch_succeeds_for_task_planning_when_ready() {
    with_mock_lm_backend(|| {
        api::safe_switch("task_planning").unwrap();
    });
}
