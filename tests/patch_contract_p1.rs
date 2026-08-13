//! P1 — Structured CodeFix patch contract (H-1 fix) integration tests.
//!
//! Drives PrimitiveExecutor's PatchCode branch through the full patch_v1
//! contract with a scripted LLM provider registered for this test binary:
//! happy path, garbage output, missing target, nonexistent target, wrong
//! target, and hallucinated context. The kernel must accept only the
//! well-formed grounded patch and reject everything else terminally.

use deterministic_ai_kernel::execution::patch_contract::PatchV1;
use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use deterministic_ai_kernel::providers;
use serde_json::json;
use std::sync::Once;

const FIXTURE_PATH: &str = "/tmp/deterministic_ai_kernel_p1_fixture_calc.rs";
const FIXTURE_CONTENT: &str = "pub fn multiply(a: i32, b: i32) -> i32 {\n    a + b\n}\n";

static FIXTURE_INIT: Once = Once::new();

/// Written exactly once per test binary: parallel tests must not race on
/// truncating/rewriting the shared fixture.
fn write_fixtures() {
    FIXTURE_INIT.call_once(|| {
        std::fs::write(FIXTURE_PATH, FIXTURE_CONTENT).unwrap();
    });
}

/// Scripted provider. It reads the kernel's own prompt to learn the target
/// file and its content (proving the prompt is grounded), then answers
/// according to a mode marker embedded in the task text:
/// - GARBAGE_MODE      -> free-form text (must be rejected)
/// - HALLUCINATE_MODE  -> patch_v1 shape but context absent from the file
/// - WRONG_TARGET_MODE -> patch_v1 shape but a different target_file
/// - otherwise         -> valid grounded patch_v1
struct ScriptedPatchLlm;

impl ScriptedPatchLlm {
    fn prompt_target(prompt: &str) -> Option<&str> {
        let start = prompt.find("FILE: ")? + "FILE: ".len();
        let end = prompt[start..].find('\n')? + start;
        Some(&prompt[start..end])
    }

    fn prompt_first_content_line(prompt: &str) -> Option<&str> {
        let start = prompt.find("<<<\n")? + "<<<\n".len();
        let rest = &prompt[start..];
        let end = rest.find('\n')?;
        Some(&rest[..end])
    }
}

impl providers::LlmProvider for ScriptedPatchLlm {
    fn coding_assistant(&self, prompt: &str) -> anyhow::Result<String> {
        self.execute_llm(prompt, None).map(|r| r.text)
    }

    fn execute_llm(
        &self,
        prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<providers::LlmResponse> {
        let target = Self::prompt_target(prompt).unwrap_or("?");
        let first_line = Self::prompt_first_content_line(prompt).unwrap_or("?");

        let text = if prompt.contains("GARBAGE_MODE") {
            "I think you should change the line to a * b, good luck!".to_string()
        } else if prompt.contains("HALLUCINATE_MODE") {
            format!(
                r#"{{"version":"patch_v1","target_file":"{target}","context_before":"code that does not exist in the file","replacement":"fixed","reason":"hallucinated context"}}"#
            )
        } else if prompt.contains("WRONG_TARGET_MODE") {
            format!(
                r#"{{"version":"patch_v1","target_file":"/tmp/some_other_file.rs","context_before":"{first_line}","replacement":"{first_line} // fixed","reason":"wrong file"}}"#
            )
        } else {
            format!(
                r#"{{"version":"patch_v1","target_file":"{target}","context_before":"{first_line}","replacement":"{first_line} // fixed","reason":"scripted valid patch"}}"#
            )
        };

        Ok(providers::LlmResponse {
            text,
            model_name: "scripted-patch-mock".to_string(),
            model_version: Some("p1".to_string()),
        })
    }

    fn embed_text(&self, _prompt: &str) -> anyhow::Result<Vec<f32>> {
        Ok(vec![0.0; 4])
    }
}

static INIT: Once = Once::new();
fn register_scripted_llm() {
    INIT.call_once(|| providers::register_llm(Box::new(ScriptedPatchLlm)));
}

fn patch_code_spec(target_file: Option<&str>, out_path: &str) -> PrimitiveSpec {
    let mut payload = json!({
        "requires_llm": true,
        "step_kind": "PatchCode",
        "path": out_path,
    });
    if let Some(t) = target_file {
        payload["target_file"] = json!(t);
    }
    PrimitiveSpec {
        id: PrimitiveId("patch-code-p1".to_string()),
        kind: PrimitiveKind::Write,
        payload,
    }
}

fn unique_out_path(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("/tmp/deterministic_ai_kernel_p1_{name}_{nanos}.patch.json")
}

#[test]
fn patch_code_happy_path_produces_validated_patch_v1() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("happy");
    let spec = patch_code_spec(Some(FIXTURE_PATH), &out);
    let task = format!("Fix the bug in {}", FIXTURE_PATH);

    let result = PrimitiveExecutor::execute("p1-happy", &spec, &task)
        .unwrap_or_else(|e| panic!("happy path must succeed, got: {e}"));

    assert_eq!(result.status, "ok");
    assert_eq!(result.output["patch_shape_validation"], "ok");
    assert_eq!(result.output["context_occurrences"], 1);
    assert_eq!(result.output["patch_target"], FIXTURE_PATH);

    let patch: PatchV1 = serde_json::from_value(result.output["patch_v1"].clone())
        .expect("patch_v1 artifact must deserialize");
    assert_eq!(patch.target_file, FIXTURE_PATH);
    assert_eq!(
        patch.context_before,
        "pub fn multiply(a: i32, b: i32) -> i32 {"
    );
    assert!(patch.replacement.ends_with("// fixed"));

    // The canonical patch JSON (not raw model text) is what gets persisted.
    let written = std::fs::read_to_string(&out).unwrap();
    let reparsed: PatchV1 = serde_json::from_str(&written).expect("persisted patch parses");
    assert_eq!(reparsed, patch);
    let _ = std::fs::remove_file(&out);

    // P1 does NOT apply the patch: the target file must be untouched.
    assert_eq!(
        std::fs::read_to_string(FIXTURE_PATH).unwrap(),
        FIXTURE_CONTENT
    );
}

#[test]
fn patch_code_rejects_garbage_model_output() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("garbage");
    let spec = patch_code_spec(Some(FIXTURE_PATH), &out);
    let task = format!("GARBAGE_MODE fix the bug in {}", FIXTURE_PATH);

    let err = PrimitiveExecutor::execute("p1-garbage", &spec, &task)
        .expect_err("free-form model text must not pass as a patch");
    assert!(
        err.to_string().starts_with("fatal: malformed patch"),
        "got: {err}"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn patch_code_rejects_missing_target() {
    register_scripted_llm();
    let out = unique_out_path("missing");
    let spec = patch_code_spec(None, &out);

    let err = PrimitiveExecutor::execute("p1-missing", &spec, "fix the bug")
        .expect_err("no resolvable target must fail");
    assert!(
        err.to_string()
            .starts_with("fatal: malformed patch: no target file resolvable"),
        "got: {err}"
    );
}

#[test]
fn patch_code_rejects_nonexistent_target() {
    register_scripted_llm();
    let out = unique_out_path("nonexistent");
    let spec = patch_code_spec(Some("/tmp/definitely_missing_p1_xyz.rs"), &out);

    let err = PrimitiveExecutor::execute("p1-nonexistent", &spec, "fix it")
        .expect_err("nonexistent target must fail");
    assert!(err.to_string().contains("does not exist"), "got: {err}");
}

#[test]
fn patch_code_rejects_wrong_target_file() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("wrongtarget");
    // Kernel-resolved target is FIXTURE_PATH; the scripted model answers
    // with a different target_file -> target mismatch must be terminal.
    let spec = patch_code_spec(Some(FIXTURE_PATH), &out);
    let task = format!("WRONG_TARGET_MODE fix the bug in {}", FIXTURE_PATH);

    let err = PrimitiveExecutor::execute("p1-wrongtarget", &spec, &task)
        .expect_err("wrong-file patch must be rejected");
    assert!(err.to_string().contains("target mismatch"), "got: {err}");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn patch_code_rejects_hallucinated_context() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("hallucinate");
    let spec = patch_code_spec(Some(FIXTURE_PATH), &out);
    let task = format!("HALLUCINATE_MODE fix the bug in {}", FIXTURE_PATH);

    let err = PrimitiveExecutor::execute("p1-hallucinate", &spec, &task)
        .expect_err("hallucinated context must be rejected");
    assert!(
        err.to_string().contains("not found in target file"),
        "got: {err}"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn read_repository_step_is_grounded_to_real_file() {
    register_scripted_llm();
    write_fixtures();
    let spec = PrimitiveSpec {
        id: PrimitiveId("read-repo-p1".to_string()),
        kind: PrimitiveKind::Read,
        payload: json!({}),
    };
    let task = format!("Read the repository for {}", FIXTURE_PATH);
    let result = PrimitiveExecutor::execute("p1-read", &spec, &task).expect("read succeeds");
    assert_eq!(result.output["path"], FIXTURE_PATH);
    assert_eq!(result.output["content"], FIXTURE_CONTENT);
}

#[test]
fn canonical_codefix_spec_patch_step_produces_patch_v1() {
    // The canonical CodeFix ExecSpec (TaskClass::CodeFix) must drive the
    // structured contract end-to-end: target resolved from the task text,
    // real file content grounding, validated patch_v1 output.
    register_scripted_llm();
    write_fixtures();
    use deterministic_ai_kernel::workflow::contract::TaskClass;
    let spec = TaskClass::CodeFix.to_exec_spec(None);
    let patch_step = spec
        .steps
        .iter()
        .find(|s| s.step_id == "02_patch_code")
        .expect("canonical CodeFix flow has a patch_code step");
    let prim = patch_step.primitive.as_ref().expect("patch primitive");
    let out = unique_out_path("canonical");
    let mut prim = prim.clone();
    prim.payload["path"] = json!(out);

    let task = format!("Fix multiply in {}", FIXTURE_PATH);
    let result = PrimitiveExecutor::execute("p1-canonical", &prim, &task)
        .unwrap_or_else(|e| panic!("canonical CodeFix patch step must succeed: {e}"));
    assert_eq!(result.output["patch_shape_validation"], "ok");
    assert_eq!(result.output["patch_target"], FIXTURE_PATH);
    assert_eq!(result.output["context_occurrences"], 1);
    let _ = std::fs::remove_file(&out);
}
