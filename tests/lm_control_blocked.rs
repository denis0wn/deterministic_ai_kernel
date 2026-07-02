use deterministic_ai_kernel::api;

#[test]
fn blocked_paths_return_errors_for_unknown_role() {
    assert!(api::dry_run_switch("unknown_role").is_err());
    assert!(api::safe_switch("unknown_role").is_err());
    assert!(api::auto_route("unknown_role").is_err());
}
