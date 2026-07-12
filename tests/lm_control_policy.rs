use deterministic_ai_kernel::lm_control::policy::switch_plan;

#[test]
fn switch_plan_is_deterministic_for_task_planning() {
    let p1 = switch_plan("task_planning", 64.0).expect("test failure");
    let p2 = switch_plan("task_planning", 64.0).expect("test failure");

    assert_eq!(p1, p2);
    assert!(
        p1.model == "mlx-community/gemma-4-12b-coder-fable5-composer2.5-4bit"
            || p1.model == "/Users/denissmoliakov/Models/gemma4-reasoning"
    );
    assert_eq!(p1.ram_class, "medium");
    assert_eq!(p1.threshold_gb, 6.0);
    assert_eq!(p1.free_gb, 64.0);
}

#[test]
fn switch_plan_preserves_injected_free_memory() {
    let low = switch_plan("task_planning", 3.5).expect("test failure");
    let high = switch_plan("task_planning", 64.0).expect("test failure");

    assert_eq!(low.model, high.model);
    assert_eq!(low.ram_class, high.ram_class);
    assert_eq!(low.threshold_gb, high.threshold_gb);
    assert_eq!(low.free_gb, 3.5);
    assert_eq!(high.free_gb, 64.0);
}
