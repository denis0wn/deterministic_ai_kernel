use deterministic_ai_kernel::api;

#[test]
fn doctor_contract_is_stable() {
    std::env::set_var("DAK_LM_BACKEND", "mock");
    let report = api::doctor_json().unwrap();

    let free_gb = report
        .get("free_gb")
        .and_then(|v| v.as_f64())
        .expect("free_gb missing");
    assert!(free_gb > 0.0);

    let lm_studio_models = report
        .get("lm_studio_models")
        .and_then(|v| v.as_u64())
        .expect("lm_studio_models missing");
    assert!(lm_studio_models >= 1);

    let roles = report
        .get("roles")
        .and_then(|v| v.as_array())
        .expect("roles missing");
    assert!(!roles.is_empty());

    let task = roles
        .iter()
        .find(|r| r.get("role").and_then(|v| v.as_str()) == Some("task_planning"))
        .expect("task_planning role missing");

    assert_eq!(
        task.get("manifest_model").and_then(|v| v.as_str()),
        Some("huihui-gemma-4-e2b-it-abliterated-mlx")
    );
    assert_eq!(
        task.get("threshold_gb").and_then(|v| v.as_f64()),
        Some(6.0)
    );
    assert_eq!(
        task.get("model_available").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        task.get("switch_ready").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        task.get("in_sync").and_then(|v| v.as_bool()),
        Some(true)
    );
}
