//! Phase 1.7 — Mock LLM Provider and Test Infrastructure
//!
//! Verifies:
//! - MockLlm provider works through the LlmProvider trait
//! - MockLlm can be registered and accessed via get_llm()
//! - LLM-dependent code paths work with the mock
//! - PrimitiveExecutor LLM branches execute correctly with mock
//! - ExecSpec with requires_llm steps work end-to-end

mod test_util;

use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use deterministic_ai_kernel::providers;
use deterministic_ai_kernel::workflow::contract::TaskClass;
use serde_json::json;

// ── 1. Mock LLM Provider Basics ────────────────────────────────────────────

#[test]
fn mock_llm_coding_assistant_returns_response() {
    test_util::register_mock_llm();
    let response = providers::get_llm()
        .coding_assistant("test prompt")
        .unwrap();
    assert!(response.starts_with("mock-coding-response-to:"));
    assert!(response.contains("test prompt"));
}

#[test]
fn mock_llm_execute_llm_returns_structured_response() {
    test_util::register_mock_llm();
    let response = providers::get_llm()
        .execute_llm("analyze this code", None)
        .unwrap();
    assert!(response.text.starts_with("mock-llm-response-to:"));
    assert_eq!(response.model_name, "mock-model-v1");
    assert_eq!(response.model_version, Some("mock-1.0".to_string()));
}

#[test]
fn mock_llm_execute_llm_respects_model_override() {
    test_util::register_mock_llm();
    let response = providers::get_llm()
        .execute_llm("test", Some("custom-model"))
        .unwrap();
    assert_eq!(response.model_name, "custom-model");
}

#[test]
fn mock_llm_embed_text_returns_deterministic_vector() {
    test_util::register_mock_llm();
    let emb1 = providers::get_llm().embed_text("hello world").unwrap();
    let emb2 = providers::get_llm().embed_text("hello world").unwrap();
    assert_eq!(emb1, emb2, "embedding must be deterministic");
    assert_eq!(emb1.len(), 128, "embedding dimension must be 128");
}

#[test]
fn mock_llm_embed_text_different_inputs_differ() {
    test_util::register_mock_llm();
    let emb1 = providers::get_llm().embed_text("hello").unwrap();
    let emb2 = providers::get_llm().embed_text("world").unwrap();
    assert_ne!(
        emb1, emb2,
        "different inputs must produce different embeddings"
    );
}

// ── 2. PrimitiveExecutor LLM Branches ──────────────────────────────────────

#[test]
fn primitive_write_with_llm_uses_mock() {
    test_util::register_mock_llm();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_str().unwrap();

    let spec = PrimitiveSpec {
        id: PrimitiveId("write-llm-test".to_string()),
        kind: PrimitiveKind::Write,
        payload: json!({"path": path, "requires_llm": true}),
    };

    let result = PrimitiveExecutor::execute("t1", &spec, "generate patch").unwrap();
    assert_eq!(result.status, "ok");

    // Verify the file was written with mock content
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.starts_with("mock-llm-response-to:"));
}

#[test]
fn primitive_compute_with_llm_uses_mock() {
    test_util::register_mock_llm();
    let spec = PrimitiveSpec {
        id: PrimitiveId("compute-llm-test".to_string()),
        kind: PrimitiveKind::Compute,
        payload: json!({"requires_llm": true, "detail": "analyze bug"}),
    };

    let result = PrimitiveExecutor::execute("t1", &spec, "payload").unwrap();
    assert_eq!(result.status, "ok");
    // The output should contain the mock LLM response
    assert!(result.output["result"]
        .as_str()
        .unwrap()
        .starts_with("mock-llm-response-to:"));
    assert_eq!(result.output["model_name"], "mock-model-v1");
}

#[test]
fn primitive_route_with_llm_uses_mock() {
    test_util::register_mock_llm();
    let spec = PrimitiveSpec {
        id: PrimitiveId("route-llm-test".to_string()),
        kind: PrimitiveKind::Route,
        payload: json!({"requires_llm": true}),
    };

    let result = PrimitiveExecutor::execute("t1", &spec, "verify patch").unwrap();
    assert_eq!(result.status, "ok");
    // Route decision goes through the fail-closed verdict gate (audit
    // findings M4/EH2). The mock LLM returns free-form text that is neither
    // PASS nor FAIL, so the gate must close (FAIL) rather than leak the raw
    // model text into the decision. The previous assertion expected the raw
    // mock string to pass through, which is precisely the fail-open hole the
    // remediation removed.
    let decision = result.output["route_decision"].as_str().unwrap();
    assert_eq!(decision, "FAIL");
}

#[test]
fn primitive_compute_semantic_embedding_uses_mock() {
    test_util::register_mock_llm();
    let spec = PrimitiveSpec {
        id: PrimitiveId("embedding-test".to_string()),
        kind: PrimitiveKind::Compute,
        payload: json!({"operation": "semantic_embedding", "detail": "analyze task"}),
    };

    let result = PrimitiveExecutor::execute("t1", &spec, "payload").unwrap();
    assert_eq!(result.status, "ok");
    assert_eq!(result.output["analysis_kind"], "semantic_seed");
    assert_eq!(result.output["vector_len"], 128);
    // Should produce an analysis_seed artifact
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].artifact_type, "analysis_seed");
}

// ── 3. Full CodeFix Pipeline with Mock LLM ─────────────────────────────────

#[test]
fn codefix_pipeline_with_mock_llm_rejects_unstructured_patch() {
    test_util::register_mock_llm();
    // P1 contract (H-1 fix): the PatchCode step must fail terminally when
    // the LLM returns free-form text instead of a patch_v1 object. The mock
    // provider only produces free-form text, so under the mock the patch
    // step is rejected.
    //
    // P2: the ApplyPatch step also fails terminally — no validated patch_v1
    // artifact exists to inject.
    //
    // P3: RunTests performs REAL authorized execution and needs an
    // authorized workspace (absent here), and ValidatePatch is a
    // deterministic evidence gate that needs injected apply/test evidence
    // (absent by construction under the mock). Both fail closed. Only the
    // two grounded read/locate steps still execute — every step that can
    // mutate or certify is fail-closed without real evidence.
    let spec = TaskClass::CodeFix.to_exec_spec(None);

    let mut executed_ok = 0;
    let mut rejected = 0;
    for step in &spec.steps {
        if let Some(ref prim) = step.primitive {
            let result = PrimitiveExecutor::execute("codefix-mock", prim, "test payload");
            let must_fail = matches!(
                step.step_id.as_str(),
                "02_patch_code" | "03_apply_patch" | "04_run_tests" | "05_validate_patch"
            );
            if must_fail {
                let err = result.expect_err("step must fail closed under the mock");
                let msg = err.to_string();
                assert!(
                    msg.starts_with("fatal:"),
                    "expected terminal failure for {}, got: {msg}",
                    step.step_id
                );
                rejected += 1;
            } else {
                result.unwrap_or_else(|e| panic!("step {} failed: {}", step.step_id, e));
                executed_ok += 1;
            }
        }
    }

    assert_eq!(
        rejected, 4,
        "patch/apply/run_tests/validate must all fail closed under a free-form mock LLM"
    );
    assert_eq!(
        executed_ok, 2,
        "only read_repository and locate_bug execute under the mock"
    );
}

// ── 4. SeedInterpreter with Mock LLM ───────────────────────────────────────

#[test]
fn seed_interpreter_works_with_mock_llm() {
    test_util::register_mock_llm();
    use deterministic_ai_kernel::workflow::contract::StepKind;
    use deterministic_ai_kernel::workflow::semantic::interpreter::SeedInterpreter;

    let domain = vec![
        StepKind::AnalyzeTask,
        StepKind::PlanExecution,
        StepKind::ExecuteChanges,
    ];

    // SeedInterpreter doesn't use LLM directly, but verify it works
    // in the same process where mock LLM is registered
    let bias = SeedInterpreter::interpret(42, &domain);
    assert_eq!(bias.preferred.len(), 3);
    assert_eq!(bias.weights.len(), 3);
}
