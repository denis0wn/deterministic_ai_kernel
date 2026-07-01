use deterministic_ai_kernel::lm_control::{doctor, list_models};

#[test]
fn live_lmstudio_is_reachable_when_explicitly_enabled() {
    if std::env::var("DAK_RUN_LIVE_LM_TESTS").ok().as_deref() != Some("1") {
        eprintln!("skipping live LM Studio test; set DAK_RUN_LIVE_LM_TESTS=1");
        return;
    }

    std::env::remove_var("DAK_LM_BACKEND");

    let models = list_models().expect("LM Studio /v1/models should respond");
    assert!(!models.is_empty(), "LM Studio returned no models");

    let report = doctor().expect("doctor() should succeed against live LM Studio");
    assert!(
        report.lm_studio_models >= 1,
        "expected at least one live model"
    );
    assert!(
        report.roles.iter().any(|r| r.role == "task_planning"),
        "task_planning role missing from doctor report"
    );
}
