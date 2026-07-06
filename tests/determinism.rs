use std::process::Command;

fn run_kernel(args: &[&str]) -> (String, bool) {
    let out = Command::new("cargo")
        .args(["run", "--quiet", "--bin", "run", "--"])
        .args(args)
        .output()
        .expect("failed to run kernel");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    (text, out.status.success())
}

#[test]
fn deterministic_core_commands_are_stable() {
    // Ядро должно стабильно отдавать help и не падать,
    // даже если семантический слой (LLM/Embeddings) недоступен или не инициализирован.
    let (out, success) = run_kernel(&["--help"]);
    assert!(success, "kernel help failed: {}", out);
    assert!(out.contains("latest-bias-artifact"));
    assert!(out.contains("analyze-task"));
}
