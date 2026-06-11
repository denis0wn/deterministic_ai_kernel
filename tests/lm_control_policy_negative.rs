use deterministic_ai_kernel::lm_control::policy::switch_plan;

#[test]
fn switch_plan_rejects_unknown_role_via_manifest_lookup() {
    let err = switch_plan("not_a_real_role", 64.0).unwrap_err();
    let msg = err.to_string();

    assert!(msg.contains("no enabled model found for role"), "{msg}");
    assert!(msg.contains("not_a_real_role"), "{msg}");
}
