mod cli_json_contract;

use cli_json_contract::assert_cli_json_v1;
use serde_json::Value;
use std::process::Command;

#[test]
fn doctor_json_cli_emits_valid_json_report() {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--", "doctor-json"])
        .output()
        .expect("failed to run doctor-json");

    assert!(
        out.status.success(),
        "doctor-json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("stdout was not valid json");

    assert_cli_json_v1(&parsed, "doctor-json");
    assert!(parsed["report"].get("free_gb").is_some());
    assert!(parsed["report"].get("local_models_count").is_some());
    assert!(parsed["report"].get("roles").is_some());
    assert!(parsed["report"]["roles"].is_array());

    if let Some(first) = parsed["report"]["roles"]
        .as_array()
        .and_then(|rows| rows.first())
    {
        assert!(first.get("role").is_some());
        assert!(first.get("manifest_model").is_some());
        assert!(first.get("env_model").is_some());
        assert!(first.get("in_sync").is_some());
        assert!(first.get("model_available").is_some());
        assert!(first.get("switch_ready").is_some());
        assert!(first.get("threshold_gb").is_some());
    }
}
