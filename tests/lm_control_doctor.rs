use deterministic_ai_kernel::lm_control::doctor;

#[test]
fn doctor_returns_roles_and_ram_state() {
    let report = doctor().unwrap();
    assert!(report.free_gb >= 0.0);
    assert!(report.lm_studio_models >= 0);
    assert!(!report.roles.is_empty());
}
