mod test_util;

use deterministic_ai_kernel::lm_control::dry_run_switch;
use test_util::with_mock_lm_backend;

#[test]
fn dry_run_switch_reports_ready_for_task_planning() {
    with_mock_lm_backend(|| {
        dry_run_switch("task_planning").expect("test failure");
    });
}
