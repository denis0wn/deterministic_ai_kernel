//! P1 — Structured CodeFix patch contract integration tests.
//!
//! Drives PrimitiveExecutor's PatchCode branch through the full patch_v1
//! contract with a scripted LLM provider registered for this test binary:
//! happy path, garbage output, missing target, nonexistent target, wrong
//! target, and hallucinated context. The kernel must accept only the
//! well-formed grounded patch and reject everything else terminally.
//!
//! Since the H-1/H-2 confinement fix every filesystem access performed by the
//! executor requires an authorized workspace, so these tests declare one. It
//! is created canonical (symlinks resolved) and exported through
//! DAK_CODEFIX_WORKSPACE, which is exactly how production authorizes it.

use deterministic_ai_kernel::execution::patch_contract::PatchV1;
use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use deterministic_ai_kernel::providers;
use serde_json::json;
use std::sync::{Once, OnceLock};

const FIXTURE_CONTENT: &str = "pub fn multiply(a: i32, b: i32) -> i32 {\n    a + b\n}\n";

/// Authorized workspace for every step in this binary.
///
/// Canonicalized on purpose: on macOS `/tmp` is a symlink to `/private/tmp`,
/// and `resolve_safe` canonicalizes, so a non-canonical workspace would make
/// every literal path comparison in these tests fail.
fn workspace() -> &'static str {
    static WS: OnceLock<String> = OnceLock::new();
    WS.get_or_init(|| {
        let dir = std::env::temp_dir().join("deterministic_ai_kernel_p1_workspace");
        std::fs::create_dir_all(&dir).expect("create p1 workspace");
        dir.canonicalize()
            .expect("canonical p1 workspace")
            .to_string_lossy()
            .into_owned()
    })
}

fn fixture_path() -> String {
    format!("{}/calc.rs", workspace())
}

static FIXTURE_INIT: Once = Once::new();

/// Written exactly once per test binary: parallel tests must not race on
/// truncating/rewriting the shared fixture. Also authorizes the workspace
/// before any step runs — `Once` blocks every other caller until this
/// completes, so no test can observe an unset DAK_CODEFIX_WORKSPACE.
fn write_fixtures() {
    FIXTURE_INIT.call_once(|| {
        std::env::set_var("DAK_CODEFIX_WORKSPACE", workspace());
        std::fs::write(fixture_path(), FIXTURE_CONTENT).unwrap();
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
                r#"{{"version":"patch_v1","target_file":"{workspace}/some_other_file.rs","context_before":"{first_line}","replacement":"{first_line} // fixed","reason":"wrong file"}}"#,
                workspace = workspace()
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
        "workspace": workspace(),
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

/// Patch output must live inside the authorized workspace: an out_path
/// outside it is now refused by confinement, which is the point of H-2.
fn unique_out_path(name: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{}/p1_{name}_{nanos}.patch.json", workspace())
}

#[test]
fn patch_code_happy_path_produces_validated_patch_v1() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("happy");
    let spec = patch_code_spec(Some(&fixture_path()), &out);
    let task = format!("Fix the bug in {}", fixture_path());

    let result = PrimitiveExecutor::execute("p1-happy", &spec, &task)
        .unwrap_or_else(|e| panic!("happy path must succeed, got: {e}"));

    assert_eq!(result.status, "ok");
    assert_eq!(result.output["patch_shape_validation"], "ok");
    assert_eq!(result.output["context_occurrences"], 1);
    assert_eq!(result.output["patch_target"], fixture_path());

    let patch: PatchV1 = serde_json::from_value(result.output["patch_v1"].clone())
        .expect("patch_v1 artifact must deserialize");
    assert_eq!(patch.target_file, fixture_path());
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
        std::fs::read_to_string(fixture_path()).unwrap(),
        FIXTURE_CONTENT
    );
}

#[test]
fn patch_code_rejects_garbage_model_output() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("garbage");
    let spec = patch_code_spec(Some(&fixture_path()), &out);
    let task = format!("GARBAGE_MODE fix the bug in {}", fixture_path());

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
    write_fixtures();
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
    write_fixtures();
    let out = unique_out_path("nonexistent");
    // Inside the workspace but absent: this must fail as "does not exist",
    // not as a confinement rejection, so the two failure modes stay distinct.
    let missing = format!("{}/definitely_missing_p1_xyz.rs", workspace());
    let spec = patch_code_spec(Some(&missing), &out);

    let err = PrimitiveExecutor::execute("p1-nonexistent", &spec, "fix it")
        .expect_err("nonexistent target must fail");
    assert!(err.to_string().contains("does not exist"), "got: {err}");
}

/// H-1: a target outside the authorized workspace must be refused before any
/// read happens. Before confinement this path was read unconditionally and its
/// content was fed into the model prompt.
#[test]
fn patch_code_rejects_target_outside_workspace() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("outside");
    let outside = std::env::temp_dir().join("deterministic_ai_kernel_p1_outside.rs");
    std::fs::write(&outside, FIXTURE_CONTENT).unwrap();
    let spec = patch_code_spec(Some(outside.to_str().unwrap()), &out);

    let err = PrimitiveExecutor::execute("p1-outside", &spec, "fix it")
        .expect_err("target outside the workspace must be refused");
    assert!(
        err.to_string().contains("confinement"),
        "expected a confinement error, got: {err}"
    );
    let _ = std::fs::remove_file(&outside);
}

/// H-2: the patch output destination is confined too, so model-influenced
/// writes cannot land outside the authorized workspace.
#[test]
fn patch_code_rejects_out_path_outside_workspace() {
    register_scripted_llm();
    write_fixtures();
    let outside_out = std::env::temp_dir().join(format!(
        "deterministic_ai_kernel_p1_escape_{}.json",
        std::process::id()
    ));
    let spec = patch_code_spec(Some(&fixture_path()), outside_out.to_str().unwrap());
    let task = format!("Fix the bug in {}", fixture_path());

    let err = PrimitiveExecutor::execute("p1-escape-write", &spec, &task)
        .expect_err("write outside the workspace must be refused");
    assert!(
        err.to_string().contains("confinement"),
        "expected a confinement error, got: {err}"
    );
    assert!(
        !outside_out.exists(),
        "nothing may be written outside the workspace"
    );
}

#[test]
fn patch_code_rejects_wrong_target_file() {
    register_scripted_llm();
    write_fixtures();
    let out = unique_out_path("wrongtarget");
    // Kernel-resolved target is the fixture; the scripted model answers
    // with a different target_file -> target mismatch must be terminal.
    let spec = patch_code_spec(Some(&fixture_path()), &out);
    let task = format!("WRONG_TARGET_MODE fix the bug in {}", fixture_path());

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
    let spec = patch_code_spec(Some(&fixture_path()), &out);
    let task = format!("HALLUCINATE_MODE fix the bug in {}", fixture_path());

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
        payload: json!({ "workspace": workspace() }),
    };
    let task = format!("Read the repository for {}", fixture_path());
    let result = PrimitiveExecutor::execute("p1-read", &spec, &task).expect("read succeeds");
    assert_eq!(result.output["path"], fixture_path());
    assert_eq!(result.output["content"], FIXTURE_CONTENT);
}

/// H-1: grounding must NOT read a path named in the task text when that path
/// lies outside the authorized workspace; the step degrades to the ungrounded
/// "repository" fallback instead of leaking the file into the prompt.
#[test]
fn read_repository_step_refuses_grounding_outside_workspace() {
    register_scripted_llm();
    write_fixtures();
    let outside = std::env::temp_dir().join("deterministic_ai_kernel_p1_secret.rs");
    std::fs::write(&outside, b"secret-outside-marker").unwrap();

    let spec = PrimitiveSpec {
        id: PrimitiveId("read-repo-p1-escape".to_string()),
        kind: PrimitiveKind::Read,
        payload: json!({ "workspace": workspace() }),
    };
    let task = format!("Read the repository for {}", outside.display());
    let result = PrimitiveExecutor::execute("p1-read-escape", &spec, &task).expect("read succeeds");

    assert_eq!(
        result.output["path"], "repository",
        "grounding must be refused, not resolved to the outside file"
    );
    assert_ne!(result.output["content"], "secret-outside-marker");
    let _ = std::fs::remove_file(&outside);
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
    prim.payload["workspace"] = json!(workspace());

    let task = format!("Fix multiply in {}", fixture_path());
    let result = PrimitiveExecutor::execute("p1-canonical", &prim, &task)
        .unwrap_or_else(|e| panic!("canonical CodeFix patch step must succeed: {e}"));
    assert_eq!(result.output["patch_shape_validation"], "ok");
    assert_eq!(result.output["patch_target"], fixture_path());
    assert_eq!(result.output["context_occurrences"], 1);
    let _ = std::fs::remove_file(&out);
}
