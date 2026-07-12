use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
struct Case {
    name: &'static str,
    pipeline: &'static str,
    reuse_pipeline: &'static str,
    env_mutation: bool,
    expect_exit: i32,
    expect_status: &'static str,
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn artifacts_dir() -> PathBuf {
    repo_root().join("artifacts")
}

fn run_graph(
    pipeline: &str,
    reuse_plan: Option<&Path>,
    verdict_out: &Path,
) -> std::process::ExitStatus {
    let mut cmd = Command::new("./scripts/run_verification_graph.sh");
    cmd.current_dir(repo_root())
        .env("DAK_FAST_TEST", "1")
        .arg("--pipeline")
        .arg(pipeline)
        .arg("--out")
        .arg(verdict_out);

    if let Some(plan) = reuse_plan {
        cmd.arg("--reuse-plan").arg(plan);
    }

    cmd.status().expect("failed to run verification graph")
}

fn mutate_plan_fingerprint(src: &Path, dst: &Path) {
    let text = fs::read_to_string(src).expect("failed to read plan");
    let mut json: serde_json::Value =
        serde_json::from_str(&text).expect("failed to parse plan json");
    json["environment_id"] = serde_json::Value::String(
        "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    );
    json["environment"]["environment_fingerprint"] = serde_json::Value::String(
        "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    );
    json["environment"]["environment_id"] = serde_json::Value::String(
        "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
    );
    json["plan_id"] = serde_json::Value::String(
        "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
    );
    json["plan_hash"] = serde_json::Value::String(
        "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
    );
    fs::write(
        dst,
        serde_json::to_string_pretty(&json).expect("test failure") + "\n",
    )
    .expect("failed to write mutated plan");
}

#[test]
fn reuse_policy_matrix() {
    let cases = vec![
        Case {
            name: "fast_same_fingerprint_allows_reuse",
            pipeline: "fast",
            reuse_pipeline: "fast",
            env_mutation: false,
            expect_exit: 0,
            expect_status: "ok",
        },
        Case {
            name: "fast_different_fingerprint_rejects_reuse",
            pipeline: "fast",
            reuse_pipeline: "fast",
            env_mutation: true,
            expect_exit: 2,
            expect_status: "invalid_reuse",
        },
        Case {
            name: "deep_same_fingerprint_allows_reuse",
            pipeline: "deep",
            reuse_pipeline: "deep",
            env_mutation: false,
            expect_exit: 0,
            expect_status: "ok",
        },
        Case {
            name: "deep_different_fingerprint_rejects_reuse",
            pipeline: "deep",
            reuse_pipeline: "deep",
            env_mutation: true,
            expect_exit: 2,
            expect_status: "invalid_reuse",
        },
        Case {
            name: "cross_pipeline_reuse_rejected_by_default",
            pipeline: "deep",
            reuse_pipeline: "fast",
            env_mutation: false,
            expect_exit: 2,
            expect_status: "invalid_reuse",
        },
    ];

    fs::create_dir_all(artifacts_dir()).expect("test failure");

    for case in cases {
        let base_plan =
            artifacts_dir().join(format!("verification_plan.{}.json", case.reuse_pipeline));
        let fake_plan = artifacts_dir().join(format!(
            "verification_plan.{}.invalid_env.json",
            case.reuse_pipeline
        ));
        let verdict = artifacts_dir().join(format!("verification_verdict.{}.json", case.name));

        let warmup = Command::new("./scripts/run_verification_graph.sh")
            .current_dir(repo_root())
            .env("DAK_FAST_TEST", "1")
            .arg("--pipeline")
            .arg(case.reuse_pipeline)
            .arg("--plan-out")
            .arg(&base_plan)
            .arg("--out")
            .arg(artifacts_dir().join(format!(
                "verification_verdict.{}.warmup.json",
                case.reuse_pipeline
            )))
            .status()
            .expect("failed to create baseline plan");

        assert!(
            warmup.success(),
            "warmup failed for case {} with status {:?}",
            case.name,
            warmup
        );

        let reuse_plan_path = if case.env_mutation {
            mutate_plan_fingerprint(&base_plan, &fake_plan);
            fake_plan.as_path()
        } else {
            base_plan.as_path()
        };

        let rerun = run_graph(case.pipeline, Some(reuse_plan_path), &verdict);

        assert_eq!(
            rerun.code(),
            Some(case.expect_exit),
            "unexpected exit code for case {}",
            case.name
        );

        let verdict_text = fs::read_to_string(&verdict)
            .unwrap_or_else(|e| panic!("failed to read verdict for case {}: {e}", case.name));
        let verdict_json: serde_json::Value = serde_json::from_str(&verdict_text)
            .unwrap_or_else(|e| panic!("failed to parse verdict for case {}: {e}", case.name));

        assert_eq!(
            verdict_json["status"], case.expect_status,
            "unexpected status for case {}",
            case.name
        );

        if case.expect_status == "ok" {
            assert_eq!(
                verdict_json["ok"], true,
                "expected ok=true for case {}",
                case.name
            );
            assert!(
                verdict_json["plan_id"].is_string(),
                "missing plan_id for case {}",
                case.name
            );
            assert!(
                verdict_json["plan_hash"].is_string(),
                "missing plan_hash for case {}",
                case.name
            );
            assert!(
                verdict_json["environment_id"].is_string(),
                "missing environment_id for case {}",
                case.name
            );
            assert!(
                verdict_json["execution_order_id"].is_string(),
                "missing execution_order_id for case {}",
                case.name
            );
        } else if case.expect_status == "invalid_reuse" {
            assert_eq!(
                verdict_json["ok"], false,
                "expected ok=false for case {}",
                case.name
            );
            assert!(
                verdict_json["reason"] == "plan_id differed: environment_id differed"
                    || verdict_json["reason"]
                        == "plan_id differed: selected_pipeline differed for reuse-plan",
                "unexpected reason for case {}: {:?}",
                case.name,
                verdict_json["reason"]
            );
        }
    }
}
