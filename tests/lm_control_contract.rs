use deterministic_ai_kernel::lm_control::{DoctorReport, DoctorRoleReport};

#[test]
fn doctor_report_serializes_stably() {
    let report = DoctorReport {
        free_gb: 12.5,
        lm_studio_models: 3,
        roles: vec![DoctorRoleReport {
            role: "coding_assistant".to_string(),
            manifest_model: "qwen-coder".to_string(),
            env_model: "qwen-coder".to_string(),
            in_sync: true,
            model_available: true,
            switch_ready: true,
            threshold_gb: 8.0,
        }],
    };

    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"free_gb\":12.5"));
    assert!(json.contains("\"lm_studio_models\":3"));
    assert!(json.contains("\"role\":\"coding_assistant\""));
    assert!(json.contains("\"switch_ready\":true"));
    assert!(json.contains("\"threshold_gb\":8.0"));
}
