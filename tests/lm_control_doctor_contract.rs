use deterministic_ai_kernel::lm_control::doctor;

#[test]
fn doctor_contract_is_stable() {
    std::env::set_var("DAK_LM_BACKEND", "mock");
    std::env::set_var("DAK_FREE_GB_OVERRIDE", "16.0");
    let report = doctor().expect("test failure");

    assert!(report.free_gb > 0.0);
    let _ = report.mlx_models; // mlx_models is usize, always valid
    assert!(!report.roles.is_empty());

    let task = report
        .roles
        .iter()
        .find(|r| r.role == "task_planning")
        .expect("task_planning role missing");

    // manifest_model should be non-empty and come from the manifest file
    assert!(
        !task.manifest_model.is_empty(),
        "manifest_model must not be empty"
    );
    assert_eq!(task.threshold_gb, 6.0);
    assert!(task.model_available);
    assert!(task.switch_ready);
    assert!(task.in_sync);
}
