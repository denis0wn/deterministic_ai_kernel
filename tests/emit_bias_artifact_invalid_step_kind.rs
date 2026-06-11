use std::process::Command;

#[test]
fn emit_bias_artifact_rejects_unknown_step_kind() {
    let output = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .args([
            "emit-bias-artifact",
            "task-invalid",
            "step-invalid",
            "AnalyzeTask",
            "Cleanup",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown step kind: Cleanup"), "{stderr}");
}
