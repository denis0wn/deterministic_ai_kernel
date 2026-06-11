use std::fs;
use std::path::Path;
use std::process::Command;

fn read_json(path: &str) -> serde_json::Value {
    let data = fs::read_to_string(path).expect("read json file");
    serde_json::from_str(&data).expect("parse json")
}

#[test]
fn verification_plan_exposes_identity_inputs() {
    let status = Command::new("./scripts/run_verification_graph.py")
        .args([
            "--pipeline",
            "fast",
            "--plan-out",
            "artifacts/verification_plan.identity_contract.json",
            "--out",
            "artifacts/verification_verdict.identity_contract.json",
        ])
        .status()
        .expect("run verification graph");

    assert!(status.success(), "runner should succeed");

    let plan_path = "artifacts/verification_plan.identity_contract.json";
    assert!(Path::new(plan_path).exists(), "plan file should exist");

    let plan = read_json(plan_path);

    assert_eq!(plan["selected_pipeline"], "fast");
    assert!(
        plan.get("plan_hash").is_some(),
        "transitional identity field plan_hash must exist"
    );

    let env = &plan["environment"];
    assert!(
        env.get("environment_fingerprint").is_some(),
        "environment.environment_fingerprint must exist as current strict identity input"
    );

    let nodes = plan["ordered_nodes"].as_array().expect("ordered_nodes array");
    assert!(!nodes.is_empty(), "plan must contain nodes");

    for node in nodes {
        assert!(node.get("key").is_some(), "node.key must exist");
        assert!(node.get("id").is_some(), "node.id must exist");
        assert!(node.get("version").is_some(), "node.version must exist");
    }
}
