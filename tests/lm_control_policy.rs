use deterministic_ai_kernel::api;

#[test]
fn switch_plan_is_deterministic_for_task_planning() {
    let p1 = api::switch_plan("task_planning", 64.0).unwrap();
    let p2 = api::switch_plan("task_planning", 64.0).unwrap();

    assert_eq!(p1, p2);
    assert_eq!(p1.model, "huihui-gemma-4-e2b-it-abliterated-mlx");
    assert_eq!(p1.ram_class, "medium");
    assert_eq!(p1.threshold_gb, 6.0);
    assert_eq!(p1.free_gb, 64.0);
}

#[test]
fn switch_plan_preserves_injected_free_memory() {
    let low = api::switch_plan("task_planning", 3.5).unwrap();
    let high = api::switch_plan("task_planning", 64.0).unwrap();

    assert_eq!(low.model, high.model);
    assert_eq!(low.ram_class, high.ram_class);
    assert_eq!(low.threshold_gb, high.threshold_gb);
    assert_eq!(low.free_gb, 3.5);
    assert_eq!(high.free_gb, 64.0);
}
