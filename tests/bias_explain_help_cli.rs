use std::process::Command;

#[test]
fn help_lists_bias_explain_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .arg("--help")
        .output()
        .expect("test failure");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("test failure");
    assert!(stdout.contains("bias-explain <step_kind>..."));
}
