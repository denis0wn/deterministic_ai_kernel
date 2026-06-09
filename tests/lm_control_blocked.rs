use deterministic_ai_kernel::lm_control::{auto_route, dry_run_switch, safe_switch};

#[test]
fn blocked_paths_return_errors_for_unknown_role() {
    assert!(dry_run_switch("unknown_role").is_err());
    assert!(safe_switch("unknown_role").is_err());
    assert!(auto_route("unknown_role").is_err());
}
