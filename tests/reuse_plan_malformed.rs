use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn artifacts_dir() -> PathBuf {
    repo_root().join("artifacts")
}

fn baseline_plan(name: &str) -> PathBuf {
    let base_plan = artifacts_dir().join(format!("verification_plan.{name}.json"));
    let warmup_verdict = artifacts_dir().join(format!("verification_verdict.{name}.warmup.json"));
    let warmup = Command::new("python3")
        .current_dir(repo_root())
        .arg("scripts/run_verification_graph.py")
        .arg("--pipeline")
        .arg("fast")
        .arg("--only")
        .arg("integrity@v1")
        .arg("--plan-out")
        .arg(&base_plan)
        .arg("--out")
        .arg(&warmup_verdict)
        .status()
        .expect("failed to create baseline plan");
    assert!(warmup.success(), "warmup failed for {name}: {warmup:?}");
    base_plan
}

fn run_graph(reuse_plan: &Path, verdict_out: &Path) -> std::process::ExitStatus {
    Command::new("python3")
        .current_dir(repo_root())
        .arg("scripts/run_verification_graph.py")
        .arg("--pipeline")
        .arg("fast")
        .arg("--reuse-plan")
        .arg(reuse_plan)
        .arg("--out")
        .arg(verdict_out)
        .status()
        .expect("failed to run verification graph")
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read json")).expect("parse json")
}

fn write_json(path: &Path, value: &Value) {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize json"),
    )
    .expect("write json");
}

fn mutate_plan(name: &str, mutate: impl FnOnce(&mut Value)) -> (PathBuf, PathBuf) {
    let baseline = baseline_plan(name);
    let mutated = artifacts_dir().join(format!("verification_plan.{name}.mutated.json"));
    let verdict = artifacts_dir().join(format!("verification_verdict.{name}.json"));
    let mut plan = read_json(&baseline);
    mutate(&mut plan);
    write_json(&mutated, &plan);
    (mutated, verdict)
}

fn assert_invalid_reuse(verdict_path: &Path, expected_error: &str, expected_reason: &str) {
    let verdict = read_json(verdict_path);
    assert_eq!(verdict["ok"], json!(false));
    assert_eq!(verdict["status"], json!("invalid_reuse"));
    assert_eq!(verdict["artifact_error"], json!(expected_error));
    assert_eq!(verdict["reason"], json!(expected_reason));
    assert_eq!(verdict["node_count"], json!(0));
    assert_eq!(verdict["nodes"], json!([]));
}

#[test]
fn missing_environment_identity_rejected() {
    let (plan, verdict) = mutate_plan("missing_environment_identity", |plan| {
        if let Some(obj) = plan.as_object_mut() {
            obj.remove("environment_id");
            obj.remove("environment_fingerprint");
            if let Some(env) = obj.get_mut("environment").and_then(|v| v.as_object_mut()) {
                env.remove("environment_id");
                env.remove("environment_fingerprint");
            }
        }
    });

    let status = run_graph(&plan, &verdict);
    assert_eq!(status.code(), Some(2), "runner failed: {status:?}");
    assert_invalid_reuse(
        &verdict,
        "MISSING_ENVIRONMENT_IDENTITY",
        "reuse plan malformed: missing environment identity",
    );
}

#[test]
fn invalid_fingerprint_rejected() {
    let (plan, verdict) = mutate_plan("invalid_environment_fingerprint", |plan| {
        plan["environment_fingerprint"] = json!("not-a-valid-sha256");
    });

    let status = run_graph(&plan, &verdict);
    assert_eq!(status.code(), Some(2), "runner failed: {status:?}");
    assert_invalid_reuse(
        &verdict,
        "INVALID_ENVIRONMENT_FINGERPRINT",
        "reuse plan malformed: invalid environment_fingerprint format",
    );
}

#[test]
fn missing_execution_order_id_rejected() {
    let (plan, verdict) = mutate_plan("missing_execution_order_id", |plan| {
        if let Some(obj) = plan.as_object_mut() {
            obj.remove("execution_order_id");
        }
    });

    let status = run_graph(&plan, &verdict);
    assert_eq!(status.code(), Some(2), "runner failed: {status:?}");
    assert_invalid_reuse(
        &verdict,
        "MISSING_EXECUTION_ORDER_ID",
        "reuse plan malformed: missing execution_order_id",
    );
}

#[test]
fn corrupted_ordered_nodes_rejected() {
    let (plan, verdict) = mutate_plan("corrupted_ordered_nodes", |plan| {
        plan["ordered_nodes"] = json!([
            {
                "key": "integrity@v1",
                "id": "integrity",
                "version": "v1"
            },
            {
                "key": "integrity@v1",
                "id": "integrity",
                "version": "v1"
            }
        ]);
    });

    let status = run_graph(&plan, &verdict);
    assert_eq!(status.code(), Some(2), "runner failed: {status:?}");
    assert_invalid_reuse(
        &verdict,
        "INVALID_ORDERED_NODES",
        "reuse plan malformed: ordered_nodes integrity failed",
    );
}

#[test]
fn schema_mismatch_rejected() {
    let (plan, verdict) = mutate_plan("schema_version_mismatch", |plan| {
        plan["graph_schema_version"] = json!(999);
    });

    let status = run_graph(&plan, &verdict);
    assert_eq!(status.code(), Some(2), "runner failed: {status:?}");
    let verdict_json = read_json(&verdict);
    assert_eq!(verdict_json["ok"], json!(false));
    assert_eq!(verdict_json["status"], json!("invalid_reuse"));
    assert_eq!(
        verdict_json["artifact_error"],
        json!("SCHEMA_VERSION_MISMATCH")
    );
    assert_eq!(
        verdict_json["reason"],
        json!("reuse plan incompatible: graph_schema_version differed")
    );
    assert_eq!(verdict_json["expected_graph_schema_version"], json!(999));
    assert_eq!(verdict_json["actual_graph_schema_version"], json!(2));
    assert_eq!(verdict_json["node_count"], json!(0));
    assert_eq!(verdict_json["nodes"], json!([]));
}
