use std::process::Command;

#[test]
fn doctor_json_contract_is_stable() {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("DAK_LM_BACKEND", "mock")
        .env("DAK_FREE_GB_OVERRIDE", "64")
        .args(["doctor-json"])
        .output()
        .expect("test failure");

    assert!(
        out.status.success(),
        "stderr=\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).expect("test failure");
    assert!(stdout.contains("\"free_gb\""), "{stdout}");
    assert!(stdout.contains("\"mlx_models\""), "{stdout}");
    assert!(stdout.contains("\"roles\""), "{stdout}");
    assert!(stdout.contains("\"task_planning\""), "{stdout}");
    assert!(
        stdout.contains(
            "\"manifest_model\":\"mlx-community/gemma-4-12b-coder-fable5-composer2.5-4bit\""
        ) || stdout.contains(
            "\"manifest_model\":\"/Users/denissmoliakov/Models/gemma4-reasoning\""
        ),
        "{stdout}"
    );
    assert!(stdout.contains("\"threshold_gb\":6.0"), "{stdout}");
    assert!(stdout.contains("\"model_available\":true"), "{stdout}");
    assert!(stdout.contains("\"switch_ready\":true"), "{stdout}");
    assert!(
        stdout.contains("\"in_sync\":true") || stdout.contains("\"in_sync\":false"),
        "{stdout}"
    );
    assert!(stdout.contains("\"embedding_status\""), "{stdout}");
}
