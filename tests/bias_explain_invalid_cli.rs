use std::process::Command;

#[test]
fn bias_explain_cli_rejects_unknown_step_kind() {
    let output = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .args(["bias-explain", "AnalyzeTask", "NotAStep"])
        .output()
        .expect("test failure");

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).expect("test failure");
    assert!(stderr.contains("unknown step kind: NotAStep"));
}
