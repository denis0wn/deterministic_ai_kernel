//! Kernel Evaluation Suite — fixture + planner harness foundation.
//!
//! Does NOT invoke MLX. Uses DAK_LM_BACKEND=mock parser path only.
//! Does NOT rewrite production planner/event/replay/primitive ABI paths.

use deterministic_ai_kernel::planner_pipeline::pipeline::Pipeline;
use deterministic_ai_kernel::planner_pipeline::{PipelineContext, Plan};
use deterministic_ai_kernel::semantic_bias::{BiasConfiguration, BiasVersion};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct EvalBudget {
    max_llm_calls: u32,
    max_tool_calls: u32,
    max_steps: u32,
}

#[derive(Debug, Deserialize)]
struct EvalOracle {
    kind: String,
    expected: Option<String>,
    path: Option<String>,
    expected_order: Option<Vec<String>>,
    required_event: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
    category: String,
    seed: u64,
    task: String,
    oracle: EvalOracle,
    budget: EvalBudget,
}

#[derive(Debug, Deserialize)]
struct EvalSuite {
    schema_version: u32,
    suite: String,
    cases: Vec<EvalCase>,
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kernel_eval/tasks.json")
}

fn load_suite() -> EvalSuite {
    let raw = std::fs::read_to_string(fixture_path()).expect("read kernel_eval fixture");
    serde_json::from_str(&raw).expect("parse kernel_eval fixture")
}

fn case_by_id<'a>(suite: &'a EvalSuite, id: &str) -> &'a EvalCase {
    suite
        .cases
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("missing evaluation case: {id}"))
}

fn eval_bias() -> BiasConfiguration {
    BiasConfiguration::new("kernel_eval", Vec::new())
}

#[test]
fn kernel_eval_fixture_schema_is_valid() {
    let suite = load_suite();
    assert_eq!(suite.schema_version, 1);
    assert_eq!(suite.suite, "kernel_eval");
    assert!(
        suite.cases.len() >= 4,
        "expected at least 4 evaluation cases"
    );

    let mut ids = HashSet::new();
    for case in &suite.cases {
        assert!(!case.id.trim().is_empty(), "empty case id");
        assert!(
            ids.insert(case.id.clone()),
            "duplicate evaluation case id: {}",
            case.id
        );
        assert!(!case.category.trim().is_empty(), "empty category");
        assert!(!case.task.trim().is_empty(), "empty task");
        assert!(
            case.seed > 0,
            "seed must be positive for replay-stable eval"
        );
        assert!(case.budget.max_steps > 0, "max_steps must be > 0");
        assert!(
            case.budget.max_llm_calls > 0,
            "max_llm_calls must be > 0 for measurable LLM budget"
        );
        assert!(
            case.budget.max_tool_calls <= case.budget.max_steps,
            "max_tool_calls must fit within max_steps budget"
        );

        match case.oracle.kind.as_str() {
            "exact_text" => {
                assert!(
                    case.oracle
                        .expected
                        .as_ref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false),
                    "exact_text oracle requires expected text"
                );
            }
            "file_exact" => {
                assert!(
                    case.oracle
                        .path
                        .as_ref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false),
                    "file_exact oracle requires path"
                );
                assert!(
                    case.oracle
                        .expected
                        .as_ref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false),
                    "file_exact oracle requires expected content"
                );
            }
            "ordered_step_tokens" => {
                let order = case
                    .oracle
                    .expected_order
                    .as_ref()
                    .expect("ordered_step_tokens requires expected_order");
                assert!(order.len() >= 2, "ordered_step_tokens needs >= 2 tokens");
            }
            "reuse_required" => {
                assert_eq!(
                    case.oracle.required_event.as_deref(),
                    Some("CACHE_HIT"),
                    "reuse_required currently expects CACHE_HIT"
                );
            }
            other => panic!("unsupported oracle kind: {other}"),
        }
    }
}

#[test]
fn kernel_eval_fixture_covers_required_categories() {
    let suite = load_suite();
    let categories: HashSet<_> = suite.cases.iter().map(|c| c.category.as_str()).collect();
    for required in ["baseline_vs_kernel", "planner", "codefix", "recovery"] {
        assert!(
            categories.contains(required),
            "missing required category: {required}"
        );
    }
}

/// Planner harness: same seed + same payload => identical plan id.
/// Uses existing Pipeline API only; mock backend avoids MLX network path.
#[test]
fn kernel_eval_planner_plan_id_is_deterministic() {
    std::env::set_var("DAK_LM_BACKEND", "mock");

    let suite = load_suite();
    let case = case_by_id(&suite, "planner_order_001");
    let pipeline = Pipeline::new(eval_bias());
    let ctx = PipelineContext {
        seed: case.seed,
        bias_version: BiasVersion::V1,
        task_id: Some(case.id.clone()),
    };

    let out1 = pipeline
        .run(case.task.clone(), &ctx)
        .expect("pipeline run 1");
    let out2 = pipeline
        .run(case.task.clone(), &ctx)
        .expect("pipeline run 2");

    assert_eq!(out1.plan.seed, case.seed);
    assert_eq!(out1.plan.id, out2.plan.id);
    assert_eq!(out1.plan.steps, out2.plan.steps);
    assert!(
        out1.plan.steps.len() as u32 <= case.budget.max_steps,
        "planner produced more steps than budget allows"
    );
    assert!(
        out1.report.passed,
        "critic must accept deterministic planner output for fixture task; violations={:?}",
        out1.report.invariant_violations
    );
}

/// Planner harness: ordered-token oracle over plan steps (parser path).
#[test]
fn kernel_eval_planner_ordered_tokens_present() {
    std::env::set_var("DAK_LM_BACKEND", "mock");

    let suite = load_suite();
    let case = case_by_id(&suite, "planner_order_001");
    let expected = case
        .oracle
        .expected_order
        .as_ref()
        .expect("planner_order_001 requires expected_order");

    let pipeline = Pipeline::new(eval_bias());
    let ctx = PipelineContext {
        seed: case.seed,
        bias_version: BiasVersion::V1,
        task_id: Some(case.id.clone()),
    };

    let out = pipeline.run(case.task.clone(), &ctx).expect("pipeline run");

    let joined = out
        .plan
        .steps
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    let mut cursor = 0usize;
    for token in expected {
        let needle = token.to_ascii_lowercase();
        if let Some(pos) = joined[cursor..].find(&needle) {
            cursor += pos + needle.len();
        } else {
            assert!(
                joined.contains(&needle),
                "missing expected planner token `{token}` in plan steps: {:?}",
                out.plan.steps
            );
        }
    }

    let rebuilt = Plan::new_with_stable_id(case.seed, out.plan.steps.clone());
    assert_eq!(rebuilt.id, out.plan.id);
}
