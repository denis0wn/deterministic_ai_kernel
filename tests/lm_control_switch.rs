mod test_util;

use deterministic_ai_kernel::api;
use test_util::with_mock_lm_backend;

#[test]
fn dry_run_switch_reports_ready_for_task_planning() {
    with_mock_lm_backend(|| {
        api::dry_run_switch("task_planning").unwrap();
    });
}
