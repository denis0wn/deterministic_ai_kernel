//! P2 — Canonical authorized patch application (H-3 apply link) integration
//! tests.
//!
//! Proves the mutation path end-to-end through the production kernel
//! pipeline: LLM proposes PatchV1 (scripted provider) → kernel validates →
//! effects loop injects the artifact into the ApplyPatch step → application
//! goes through the authorized tool boundary → the file REALLY changes →
//! kernel-owned hash evidence is persisted. All workspaces are disposable
//! /tmp directories; the user's repository is never touched.

use deterministic_ai_kernel::effects::execute_effects;
use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::providers;
use deterministic_ai_kernel::workflow::contract::{steps_to_exec_spec, Step, StepKind};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Once;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_suffix(name: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{name}_{n}_{nanos}")
}

fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(unique_suffix(name));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const ORIGINAL: &str = "pub fn multiply(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
const APPLIED: &str = "pub fn multiply(a: i32, b: i32) -> i32 { // fixed\n    a + b\n}\n";

// P3 full-chain fixture (Python so a REAL allowlisted test command can run).
const PY_ORIGINAL: &str = "def multiply(a, b):\n    return a + b\n";
const PY_APPLIED: &str = "def multiply(a, b):\n    return a * b\n";
const PY_TEST: &str =
    "from calc import multiply\nassert multiply(2, 3) == 6\nprint('PY_TEST_OK')\n";

/// Scripted provider: answers PatchCode prompts with a grounded, valid
/// patch_v1 object (target + context read from the kernel's own prompt).
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
        let text = format!(
            r#"{{"version":"patch_v1","target_file":"{target}","context_before":"{first_line}","replacement":"{first_line} // fixed","reason":"scripted valid patch"}}"#
        );
        Ok(providers::LlmResponse {
            text,
            model_name: "scripted-patch-mock".to_string(),
            model_version: Some("p2".to_string()),
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

/// Create a task row with the given spec and a pipeline_input artifact so
/// the effects loop can resolve the payload (mirrors main.rs pipeline-run).
fn create_task(task_id: &str, spec_json: &str, payload: &str) -> String {
    let db = std::env::temp_dir().join(format!("{}.db", unique_suffix("dak_p2_task")));
    let db_str = db.to_string_lossy().into_owned();
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db_str).unwrap();
    conn.execute(
        "INSERT INTO tasks (task_id, task_class, exec_spec) VALUES (?1, 'CodeFix', ?2)",
        rusqlite::params![task_id, spec_json],
    )
    .unwrap();
    drop(conn);
    std::fs::create_dir_all("artifacts").unwrap();
    std::fs::write(format!("artifacts/pipeline_input.{task_id}.txt"), payload).unwrap();
    db_str
}

fn cleanup_task_files(task_id: &str, db_str: &str) {
    let _ = std::fs::remove_file(format!("artifacts/pipeline_input.{task_id}.txt"));
    let _ = std::fs::remove_file(db_str);
    let _ = std::fs::remove_file(format!("{db_str}-wal"));
    let _ = std::fs::remove_file(format!("{db_str}-shm"));
}

/// ExecSpec with PatchCode → ApplyPatch, workspace injected by the kernel
/// (test-authored spec data, never LLM input).
fn patch_then_apply_spec(workspace: &str) -> serde_json::Value {
    let mut spec = steps_to_exec_spec(&[
        Step {
            kind: StepKind::PatchCode,
            detail: Some("patch code".to_string()),
        },
        Step {
            kind: StepKind::ApplyPatch,
            detail: Some("apply patch".to_string()),
        },
    ]);
    // steps: 00_patch_code, 01_apply_patch
    let apply = spec
        .steps
        .iter_mut()
        .find(|s| s.step_id == "01_apply_patch")
        .unwrap();
    apply
        .primitive
        .as_mut()
        .unwrap()
        .payload
        .as_object_mut()
        .unwrap()
        .insert("workspace".to_string(), json!(workspace));
    serde_json::to_value(&spec).unwrap()
}

#[test]
fn e2e_apply_mutates_file_with_kernel_owned_evidence() {
    // P3 full chain (deterministic, no LLM): a valid patch_v1 is seeded into
    // the canonical artifact store exactly as a PatchCode step would persist
    // it, then ApplyPatch → REAL RunTests → ValidatePatch must all succeed
    // and completion is only allowed on real evidence:
    //   apply mutates the file → the allowlisted test command really runs
    //   and exits 0 → the deterministic validation gate passes.
    register_scripted_llm();
    let ws = fresh_dir("dak_p2_ws");
    let calc = ws.join("calc.py");
    let test_file = ws.join("test_calc.py");
    std::fs::write(&calc, PY_ORIGINAL).unwrap();
    std::fs::write(&test_file, PY_TEST).unwrap();
    let payload = format!("Fix multiply in {}", calc.display());
    let task_id = unique_suffix("p2-e2e");

    // Spec: ApplyPatch → RunTests → ValidatePatch, workspace kernel-owned.
    let mut spec = steps_to_exec_spec(&[
        Step {
            kind: StepKind::ApplyPatch,
            detail: Some("apply patch".to_string()),
        },
        Step {
            kind: StepKind::RunTests,
            detail: Some("run tests".to_string()),
        },
        Step {
            kind: StepKind::ValidatePatch,
            detail: Some("validate patch".to_string()),
        },
    ]);
    for step_id in ["00_apply_patch", "01_run_tests"] {
        let step = spec
            .steps
            .iter_mut()
            .find(|s| s.step_id == step_id)
            .unwrap();
        step.primitive
            .as_mut()
            .unwrap()
            .payload
            .as_object_mut()
            .unwrap()
            .insert("workspace".to_string(), json!(ws.to_str().unwrap()));
    }

    let db = create_task(&task_id, &serde_json::to_string(&spec).unwrap(), &payload);

    // Seed the validated patch_v1 artifact (kernel-to-kernel handoff, the
    // same shape PatchCode produces in the production flow).
    let bus = EventBus::new(&db).unwrap();
    let generation = bus.latest_generation_for_task(&task_id).unwrap_or(0);
    let patch_output = json!({
        "patch_v1": {
            "version": "patch_v1",
            "target_file": calc.to_str().unwrap(),
            "context_before": "    return a + b",
            "replacement": "    return a * b",
            "reason": "multiply must multiply"
        }
    });
    bus.append_semantic_artifact(
        &task_id,
        "00_patch_code",
        generation,
        "primitive_result_v1",
        &patch_output,
    )
    .unwrap();

    execute_effects(&db, &task_id).expect("pipeline must complete the full chain");

    // 1. The file REALLY changed (mission: prove mutation, not status).
    assert_eq!(std::fs::read_to_string(&calc).unwrap(), PY_APPLIED);

    // 2. Kernel-owned apply evidence with differing pre/post hashes.
    let artifacts = bus.list_semantic_artifacts(&task_id, None).unwrap();
    let evidence = artifacts
        .iter()
        .rev()
        .find_map(|row| {
            let payload: serde_json::Value = serde_json::from_str(&row.payload).ok()?;
            if payload.get("evidence").is_some() {
                Some(payload)
            } else {
                None
            }
        })
        .expect("apply evidence artifact must exist");
    let ev = &evidence["evidence"];
    assert_eq!(evidence["status"], "applied");
    assert_eq!(evidence["tool"], "apply_patch_v1");
    assert_ne!(
        ev["pre_image_blake3"], ev["post_image_blake3"],
        "pre/post hashes must differ"
    );
    assert_eq!(
        ev["post_image_blake3"],
        blake3::hash(PY_APPLIED.as_bytes()).to_hex().to_string(),
        "post hash must be computed from real file state"
    );
    assert_eq!(ev["pre_image_bytes"], PY_ORIGINAL.len());
    assert_eq!(ev["patch_version"], "patch_v1");

    // 3. REAL test_report_v1 from actual execution (never LLM-authored).
    let report = artifacts
        .iter()
        .rev()
        .find_map(|row| {
            let payload: serde_json::Value = serde_json::from_str(&row.payload).ok()?;
            payload.get("test_report_v1").cloned()
        })
        .expect("test_report_v1 artifact must exist");
    assert_eq!(report["passed"], true);
    assert_eq!(report["exit_code"], 0);
    assert_eq!(report["version"], "test_report_v1");
    assert_eq!(report["command_id"], "python_test_file");
    assert!(
        report["stdout_tail"]
            .as_str()
            .unwrap_or("")
            .contains("PY_TEST_OK"),
        "real test stdout must be captured: {:?}",
        report["stdout_tail"]
    );

    // 4. Deterministic validation gate decision.
    let gate = artifacts
        .iter()
        .rev()
        .find_map(|row| {
            let payload: serde_json::Value = serde_json::from_str(&row.payload).ok()?;
            if payload.get("gate").is_some() {
                Some(payload)
            } else {
                None
            }
        })
        .expect("validate_patch gate artifact must exist");
    assert_eq!(gate["route_decision"], "PASS");
    assert_eq!(gate["gate"], "deterministic_evidence");

    // 5. Effect ledger committed all three step effects.
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db).unwrap();
    let committed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM effect_ledger WHERE task_id = ?1 AND state = 'committed'",
            rusqlite::params![task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(committed, 3, "all three steps' effects must be committed");

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn e2e_completion_blocked_when_real_tests_fail() {
    // P3 matrix J: patch applies cleanly but the real test FAILS after the
    // mutation → the task must NOT complete; truthful terminal failure.
    register_scripted_llm();
    let ws = fresh_dir("dak_p3_fail_ws");
    let calc = ws.join("calc.py");
    let test_file = ws.join("test_calc.py");
    std::fs::write(&calc, PY_ORIGINAL).unwrap();
    // Test expects a result the correct fix cannot produce.
    std::fs::write(
        &test_file,
        "from calc import multiply\nassert multiply(2, 3) == 7\n",
    )
    .unwrap();
    let payload = format!("Fix multiply in {}", calc.display());
    let task_id = unique_suffix("p3-fail");

    let mut spec = steps_to_exec_spec(&[
        Step {
            kind: StepKind::ApplyPatch,
            detail: Some("apply patch".to_string()),
        },
        Step {
            kind: StepKind::RunTests,
            detail: Some("run tests".to_string()),
        },
        Step {
            kind: StepKind::ValidatePatch,
            detail: Some("validate patch".to_string()),
        },
    ]);
    for step_id in ["00_apply_patch", "01_run_tests"] {
        let step = spec
            .steps
            .iter_mut()
            .find(|s| s.step_id == step_id)
            .unwrap();
        step.primitive
            .as_mut()
            .unwrap()
            .payload
            .as_object_mut()
            .unwrap()
            .insert("workspace".to_string(), json!(ws.to_str().unwrap()));
    }
    let db = create_task(&task_id, &serde_json::to_string(&spec).unwrap(), &payload);
    let bus = EventBus::new(&db).unwrap();
    let generation = bus.latest_generation_for_task(&task_id).unwrap_or(0);
    bus.append_semantic_artifact(
        &task_id,
        "00_patch_code",
        generation,
        "primitive_result_v1",
        &json!({
            "patch_v1": {
                "version": "patch_v1",
                "target_file": calc.to_str().unwrap(),
                "context_before": "    return a + b",
                "replacement": "    return a * b",
                "reason": "correct fix but the test is wrong"
            }
        }),
    )
    .unwrap();

    let err = execute_effects(&db, &task_id).expect_err("failing tests must block completion");
    assert!(err.to_string().contains("real tests failed"), "got: {err}");
    // The file was mutated by the (valid) patch, but the task is NOT
    // completed — truthful failure, no fabricated success.
    assert_eq!(std::fs::read_to_string(&calc).unwrap(), PY_APPLIED);
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db).unwrap();
    let state: String = conn
        .query_row(
            "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = '01_run_tests'",
            rusqlite::params![task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        state, "rejected",
        "run_tests step must be terminally rejected"
    );

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn e2e_stale_file_between_generation_and_apply_is_rejected() {
    register_scripted_llm();
    let ws = fresh_dir("dak_p2_ws_stale");
    let fixture = ws.join("calc.rs");
    std::fs::write(&fixture, ORIGINAL).unwrap();
    let payload = format!("Fix multiply in {}", fixture.display());

    // Step 1: generate the patch through the real PatchCode executor.
    let gen_spec = steps_to_exec_spec(&[Step {
        kind: StepKind::PatchCode,
        detail: Some("patch code".to_string()),
    }]);
    let prim = gen_spec.steps[0].primitive.as_ref().unwrap();
    let result = PrimitiveExecutor::execute("p2-stale-gen", prim, &payload)
        .expect("patch generation succeeds");
    assert!(result.output.get("patch_v1").is_some());

    // Step 2: persist the patch artifact exactly like the effects loop does.
    let task_id = unique_suffix("p2-stale");
    let mut apply_spec = steps_to_exec_spec(&[Step {
        kind: StepKind::ApplyPatch,
        detail: Some("apply patch".to_string()),
    }]);
    apply_spec.steps[0]
        .primitive
        .as_mut()
        .unwrap()
        .payload
        .as_object_mut()
        .unwrap()
        .insert("workspace".to_string(), json!(ws.to_str().unwrap()));
    let db = create_task(
        &task_id,
        &serde_json::to_string(&apply_spec).unwrap(),
        &payload,
    );
    let bus = EventBus::new(&db).unwrap();
    let generation = bus.latest_generation_for_task(&task_id).unwrap_or(0);
    bus.append_semantic_artifact(
        &task_id,
        "02_patch_code",
        generation,
        "primitive_result_v1",
        &result.output,
    )
    .unwrap();

    // Step 3: another process modifies the PATCHED REGION (the first line,
    // which is the scripted patch's context) BEFORE apply.
    std::fs::write(
        &fixture,
        "pub fn multiply(x: i32, y: i32) -> i32 {\n    x.wrapping_mul(y)\n}\n",
    )
    .unwrap();

    // Step 4: apply must FAIL CLOSED — stale patch is refused.
    let err = execute_effects(&db, &task_id).expect_err("stale apply must fail");
    assert!(
        err.to_string().contains("apply precondition failed"),
        "got: {err}"
    );
    // The externally-modified content must be untouched by the kernel.
    assert_eq!(
        std::fs::read_to_string(&fixture).unwrap(),
        "pub fn multiply(x: i32, y: i32) -> i32 {\n    x.wrapping_mul(y)\n}\n"
    );

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn e2e_target_outside_workspace_is_rejected_by_pipeline() {
    register_scripted_llm();
    let ws = fresh_dir("dak_p2_ws_confined"); // authorized workspace (empty)
    let outside = fresh_dir("dak_p2_outside");
    let fixture = outside.join("calc.rs");
    std::fs::write(&fixture, ORIGINAL).unwrap();

    let payload = format!("Fix multiply in {}", fixture.display());
    let task_id = unique_suffix("p2-escape");
    let spec = patch_then_apply_spec(ws.to_str().unwrap());
    let db = create_task(&task_id, &spec.to_string(), &payload);

    let err = execute_effects(&db, &task_id).expect_err("workspace escape must fail");
    assert!(err.to_string().contains("escapes workspace"), "got: {err}");
    // No mutation outside the workspace.
    assert_eq!(std::fs::read_to_string(&fixture).unwrap(), ORIGINAL);

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn e2e_apply_without_authorized_workspace_fails_closed() {
    register_scripted_llm();
    let ws = fresh_dir("dak_p2_ws_missing");
    let fixture = ws.join("calc.rs");
    std::fs::write(&fixture, ORIGINAL).unwrap();
    let payload = format!("Fix multiply in {}", fixture.display());

    // Spec WITHOUT the kernel-owned workspace field.
    let spec = steps_to_exec_spec(&[
        Step {
            kind: StepKind::PatchCode,
            detail: Some("patch code".to_string()),
        },
        Step {
            kind: StepKind::ApplyPatch,
            detail: Some("apply patch".to_string()),
        },
    ]);
    let task_id = unique_suffix("p2-nows");
    let db = create_task(&task_id, &serde_json::to_string(&spec).unwrap(), &payload);

    // No DAK_CODEFIX_WORKSPACE in this test binary → fail closed.
    std::env::remove_var("DAK_CODEFIX_WORKSPACE");
    let err = execute_effects(&db, &task_id).expect_err("missing workspace must fail");
    assert!(
        err.to_string().contains("no authorized workspace"),
        "got: {err}"
    );
    assert_eq!(std::fs::read_to_string(&fixture).unwrap(), ORIGINAL);

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn tool_gate_requires_confirmation_for_apply_patch_v1() {
    let ws = fresh_dir("dak_p2_gate");
    let fixture = ws.join("calc.rs");
    std::fs::write(&fixture, ORIGINAL).unwrap();

    let args = json!({
        "patch": {
            "version": "patch_v1",
            "target_file": fixture.to_str().unwrap(),
            "context_before": "    a + b",
            "replacement": "    a * b",
            "reason": "gate test"
        }
    });

    let rt = tokio::runtime::Runtime::new().unwrap();
    // Unconfirmed mutating invocation must be refused by the gate.
    let denied = rt.block_on(deterministic_ai_kernel::tools::registry::execute_tool(
        "apply_patch_v1",
        &args,
        ws.to_str().unwrap(),
        false,
    ));
    assert!(!denied.success);
    assert!(
        denied
            .error
            .unwrap_or_default()
            .contains("authorization required"),
        "gate must refuse unconfirmed application"
    );
    assert_eq!(std::fs::read_to_string(&fixture).unwrap(), ORIGINAL);

    // Unknown tool names stay fail-closed.
    let unknown = rt.block_on(deterministic_ai_kernel::tools::registry::execute_tool(
        "apply_patch_v2",
        &args,
        ws.to_str().unwrap(),
        true,
    ));
    assert!(!unknown.success);
    assert!(unknown.error.unwrap_or_default().contains("unknown tool"));

    let _ = std::fs::remove_dir_all(&ws);
}
