use std::process::Command;

#[test]
fn doctor_json_contract_is_stable() {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("DAK_LM_BACKEND", "mock")
        .args(["doctor-json"])
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\"free_gb\""), "{stdout}");
    assert!(stdout.contains("\"lm_studio_models\""), "{stdout}");
    assert!(stdout.contains("\"roles\""), "{stdout}");
    assert!(stdout.contains("\"task_planning\""), "{stdout}");
    assert!(
        stdout.contains("\"manifest_model\": \"huihui-gemma-4-e2b-it-abliterated-mlx\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"threshold_gb\": 6.0"), "{stdout}");
    assert!(stdout.contains("\"model_available\": true"), "{stdout}");
    assert!(stdout.contains("\"switch_ready\": true"), "{stdout}");
    assert!(stdout.contains("\"in_sync\": true"), "{stdout}");
}
