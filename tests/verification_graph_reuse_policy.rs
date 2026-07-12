use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn verification_graph_rejects_invalid_reuse_when_environment_fingerprint_differs() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let artifacts_dir = repo_root.join("artifacts");
    let plan_path = artifacts_dir.join("verification_plan.json");
    let fake_plan_path = artifacts_dir.join("verification_plan.invalid_env.json");
    let verdict_path = artifacts_dir.join("verification_verdict.invalid_reuse.json");

    fs::create_dir_all(&artifacts_dir).expect("test failure");

    let warmup = Command::new("./scripts/run_verification_graph.sh")
        .current_dir(repo_root)
        .env("DAK_FAST_TEST", "1")
        .args(["--pipeline", "fast"])
        .status()
        .expect("failed to run verification graph warmup");

    assert!(
        warmup.success(),
        "warmup run failed with status: {warmup:?}"
    );

    let plan_text = fs::read_to_string(&plan_path).expect("failed to read verification_plan.json");
    let mut plan_json: serde_json::Value =
        serde_json::from_str(&plan_text).expect("failed to parse verification_plan.json");

    plan_json["environment"]["environment_fingerprint"] = serde_json::Value::String(
        "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    );

    fs::write(
        &fake_plan_path,
        serde_json::to_string_pretty(&plan_json).expect("test failure") + "\n",
    )
    .expect("failed to write fake reuse plan");

    let rerun = Command::new("./scripts/run_verification_graph.sh")
        .current_dir(repo_root)
        .env("DAK_FAST_TEST", "1")
        .args([
            "--pipeline",
            "fast",
            "--reuse-plan",
            fake_plan_path.to_str().expect("test failure"),
            "--out",
            verdict_path.to_str().expect("test failure"),
        ])
        .status()
        .expect("failed to run verification graph with reuse plan");

    assert_eq!(rerun.code(), Some(2), "expected exit code 2, got {rerun:?}");

    let verdict_text =
        fs::read_to_string(&verdict_path).expect("failed to read invalid reuse verdict");
    let verdict_json: serde_json::Value =
        serde_json::from_str(&verdict_text).expect("failed to parse invalid reuse verdict");

    assert_eq!(verdict_json["status"], "invalid_reuse");
    assert_eq!(verdict_json["ok"], false);
    assert_eq!(
        verdict_json["reason"],
        "plan_hash matched but environment_fingerprint differed"
    );
}
