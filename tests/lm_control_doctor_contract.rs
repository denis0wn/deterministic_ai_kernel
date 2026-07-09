use deterministic_ai_kernel::lm_control::doctor;

#[test]
fn doctor_contract_is_stable() {
    std::env::set_var("DAK_LM_BACKEND", "mock");
    std::env::set_var("DAK_FREE_GB_OVERRIDE", "16.0");
    let report = doctor().unwrap();

    assert!(report.free_gb > 0.0);
    assert!(report.lm_studio_models >= 1);
    assert!(!report.roles.is_empty());

    let task = report
        .roles
        .iter()
        .find(|r| r.role == "task_planning")
        .expect("task_planning role missing");

    assert_eq!(task.manifest_model, "google/gemma-4-12b-qat");
    assert_eq!(task.threshold_gb, 7.5);
    assert!(task.model_available);
    assert!(task.switch_ready);
    assert!(task.in_sync);
}
