use std::process::Command;

#[test]
fn doctor_json_contract_is_stable() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"));
    cmd.env("DAK_LM_BACKEND", "mock")
        .env("DAK_FREE_GB_OVERRIDE", "64");
    // Hermetic sync: fresh checkouts (CI) have no .env; pin role env keys
    // from the manifest so in_sync reflects the manifest, not the host.
    for role in [
        "coding_assistant",
        "task_planning",
        "code_review",
        "embeddings",
    ] {
        let key =
            deterministic_ai_kernel::model_manifest::env_key_for_role(role).expect("test failure");
        let model = deterministic_ai_kernel::model_manifest::best_enabled_model_for_role(role)
            .expect("test failure");
        cmd.env(key, model.id);
    }
    let out = cmd.args(["doctor-json"]).output().expect("test failure");

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
        stdout.contains("\"manifest_model\":\""),
        "manifest_model must be non-empty in JSON output: {stdout}"
    );
    assert!(stdout.contains("\"threshold_gb\":6.0"), "{stdout}");
    assert!(stdout.contains("\"model_available\":true"), "{stdout}");
    assert!(stdout.contains("\"switch_ready\":true"), "{stdout}");
    assert!(stdout.contains("\"in_sync\":true"), "{stdout}");
}
