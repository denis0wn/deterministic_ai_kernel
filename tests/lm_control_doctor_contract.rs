use deterministic_ai_kernel::lm_control::doctor;
use deterministic_ai_kernel::model_manifest;

#[test]
fn doctor_contract_is_stable() {
    std::env::set_var("DAK_LM_BACKEND", "mock");
    std::env::set_var("DAK_FREE_GB_OVERRIDE", "16.0");
    // Hermetic sync: doctor() compares role env keys against the manifest.
    // Fresh checkouts (CI) have no .env, so pin process env from the manifest.
    for role in ["coding_assistant", "task_planning", "code_review", "embeddings"] {
        let key = model_manifest::env_key_for_role(role).expect("test failure");
        let model = model_manifest::best_enabled_model_for_role(role).expect("test failure");
        std::env::set_var(key, model.id);
    }
    let report = doctor().expect("test failure");

    assert!(report.free_gb > Some(0.0));
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
