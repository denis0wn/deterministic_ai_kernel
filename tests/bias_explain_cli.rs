use std::process::Command;

#[test]
fn bias_explain_cli_prints_stable_lines() {
    let output = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .args(["bias-explain", "AnalyzeTask", "ExecuteChanges"])
        .output()
        .unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let expected = [
        "bias.version=v1",
        "bias.seed=0",
        "bias.preferred=[AnalyzeTask, ExecuteChanges]",
        "bias.meta.version=v1",
        "bias.meta.seed=0",
        "bias.meta.preferred_count=2",
        "bias.meta.weighted_count=2",
        "bias.weight.AnalyzeTask=1.000000",
        "bias.weight.ExecuteChanges=1.000000",
    ]
    .join("\n");

    assert_eq!(stdout.trim(), expected);
}
