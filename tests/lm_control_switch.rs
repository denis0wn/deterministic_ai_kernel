use deterministic_ai_kernel::lm_control::dry_run_switch;

#[test]
fn dry_run_switch_reports_ready_for_task_planning() {
    dry_run_switch("task_planning").unwrap();
}
