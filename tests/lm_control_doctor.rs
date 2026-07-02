use deterministic_ai_kernel::api;

#[test]
fn doctor_returns_roles_and_ram_state() {
    let report = api::doctor_json().unwrap();
    let free_gb = report
        .get("free_gb")
        .and_then(|v| v.as_f64())
        .expect("free_gb missing from doctor report");
    let roles = report
        .get("roles")
        .and_then(|v| v.as_array())
        .expect("roles missing from doctor report");
    assert!(free_gb >= 0.0);
    assert!(!roles.is_empty());
}
