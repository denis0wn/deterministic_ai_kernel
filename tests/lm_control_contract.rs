use std::process::Command;

fn run(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("DAK_LM_BACKEND", "mock")
        .args(args)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stdout=\n{}\n\nstderr=\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn dry_run_switch_output_schema_is_stable() {
    let stdout = run(&["switch", "task_planning", "--dry-run"]);

    assert!(stdout.contains("DRY_RUN_ROLE=task_planning"), "{stdout}");
    assert!(stdout.contains("DRY_RUN_MODEL="), "{stdout}");
    assert!(stdout.contains("DRY_RUN_RAM_CLASS="), "{stdout}");
    assert!(stdout.contains("DRY_RUN_THRESHOLD_GB="), "{stdout}");
    assert!(stdout.contains("DRY_RUN_FREE_GB="), "{stdout}");
    assert!(stdout.contains("DRY_RUN_MODEL_AVAILABLE="), "{stdout}");
    assert!(
        stdout.contains("DRY_RUN_WOULD_WRITE=OPENAI_MODEL_TASK_PLANNING="),
        "{stdout}"
    );
    assert!(stdout.contains("DRY_RUN_OK_TO_SWITCH="), "{stdout}");
}

#[test]
fn safe_switch_output_schema_is_stable() {
    let stdout = run(&["switch", "task_planning"]);

    assert!(stdout.contains("SAFE_SWITCH_OK"), "{stdout}");
    assert!(stdout.contains("role=task_planning"), "{stdout}");
    assert!(stdout.contains("model="), "{stdout}");
    assert!(stdout.contains("ram_class="), "{stdout}");
    assert!(stdout.contains("threshold_gb="), "{stdout}");
    assert!(stdout.contains("free_gb="), "{stdout}");
}
