use deterministic_ai_kernel::lm_control::auto_route;

#[test]
fn auto_route_succeeds_for_task_planning_when_ready() {
    auto_route("task_planning").unwrap();
}
