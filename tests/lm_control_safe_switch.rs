use deterministic_ai_kernel::lm_control::safe_switch;

#[test]
fn safe_switch_succeeds_for_task_planning_when_ready() {
    safe_switch("task_planning").unwrap();
}
