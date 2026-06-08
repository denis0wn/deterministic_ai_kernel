use deterministic_ai_kernel::lm_control::{dry_run_switch, safe_switch, auto_route};

#[test]
fn blocked_paths_return_errors_for_unknown_role() {
    assert!(dry_run_switch("unknown_role").is_err());
    assert!(safe_switch("unknown_role").is_err());
    assert!(auto_route("unknown_role").is_err());
}
