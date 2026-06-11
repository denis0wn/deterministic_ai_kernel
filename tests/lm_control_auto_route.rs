mod test_util;

use deterministic_ai_kernel::lm_control::auto_route;
use test_util::with_mock_lm_backend;

/// Uses mock LM backend and should run in the default test suite.
#[test]
fn auto_route_succeeds_for_task_planning_when_ready() {
    with_mock_lm_backend(|| {
        auto_route("task_planning").unwrap();
    });
}
