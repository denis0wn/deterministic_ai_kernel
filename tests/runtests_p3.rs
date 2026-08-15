//! P3 — REAL RunTests + completion gate integration tests.
//!
//! Complements the unit tests in src/tools/test_runner.rs with
//! executor/effect-loop level proofs:
//! - LLM-supplied command strings are NEVER executed (matrix F);
//! - completion without test evidence is impossible (matrix H);
//! - concurrent CodeFix workloads do not cross-contaminate (matrix O);
//! - kernel evidence is deterministic for identical inputs;
//! - the run_tests_v1 tool is confirmation-gated like other mutating tools.

use deterministic_ai_kernel::effects::execute_effects;
use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use deterministic_ai_kernel::workflow::contract::{steps_to_exec_spec, Step, StepKind};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique(name: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{name}_{n}_{nanos}")
}

fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(unique(name));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn create_task(task_id: &str, spec_json: &str, payload: &str) -> String {
    let db = std::env::temp_dir().join(format!("{}.db", unique("dak_p3_task")));
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

fn python_project(dir: &std::path::Path, test_asserts: &str) {
    std::fs::write(
        dir.join("calc.py"),
        "def multiply(a, b):\n    return a + b\n",
    )
    .unwrap();
    std::fs::write(dir.join("test_calc.py"), test_asserts).unwrap();
}

fn seed_patch(db: &str, task_id: &str, target: &str) {
    let bus = EventBus::new(db).unwrap();
    let generation = bus.latest_generation_for_task(task_id).unwrap_or(0);
    bus.append_semantic_artifact(
        task_id,
        "00_patch_code",
        generation,
        "primitive_result_v1",
        &json!({
            "patch_v1": {
                "version": "patch_v1",
                "target_file": target,
                "context_before": "    return a + b",
                "replacement": "    return a * b",
                "reason": "p3 test patch"
            }
        }),
    )
    .unwrap();
}

fn full_chain_spec(workspace: &str) -> serde_json::Value {
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
            .insert("workspace".to_string(), json!(workspace));
    }
    serde_json::to_value(&spec).unwrap()
}

#[test]
fn llm_supplied_command_strings_are_never_executed() {
    // Matrix F: the RunTests primitive payload may carry arbitrary
    // LLM-originated fields ("command", "script", "validation.command").
    // None of them may influence what runs: the command is derived by the
    // kernel from workspace inspection only.
    let ws = fresh_dir("p3_noshell");
    // Note: multiply returns a+b here, so the assertion matches the BUGGY
    // behavior on purpose — this test proves command derivation/security,
    // not the fix.
    python_project(
        &ws,
        "from calc import multiply\nassert multiply(2, 3) == 5\n",
    );

    let marker = ws.join("PWNED");
    let spec = PrimitiveSpec {
        id: PrimitiveId("run-tests-hostile".to_string()),
        kind: PrimitiveKind::Compute,
        payload: json!({
            "step_kind": "RunTests",
            "workspace": ws.to_str().unwrap(),
            "timeout_secs": 30,
            // Hostile LLM-supplied fields — must all be ignored:
            "command": format!("touch {}", marker.display()),
            "script": format!("rm -rf {}", ws.display()),
            "validation": { "command": format!("touch {}.x", marker.display()) }
        }),
    };

    let result = PrimitiveExecutor::execute("p3-hostile", &spec, "hostile payload")
        .expect("real tests must run and pass");
    assert_eq!(result.output["tests_passed"], true);
    assert_eq!(
        result.output["test_report_v1"]["command_id"], "python_test_file",
        "command must be workspace-derived, never payload-derived"
    );
    assert!(
        !marker.exists(),
        "LLM-supplied command must NOT have been executed"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn completion_without_test_evidence_is_impossible() {
    // Matrix H: a CodeFix spec that applies a patch but never runs real
    // tests must NOT complete — the deterministic ValidatePatch gate refuses
    // missing evidence, and the defense-in-depth check backs it up.
    let ws = fresh_dir("p3_noev");
    python_project(
        &ws,
        "from calc import multiply\nassert multiply(2, 3) == 6\n",
    );
    let calc = ws.join("calc.py");

    // Spec WITHOUT the run_tests step.
    let mut spec = steps_to_exec_spec(&[
        Step {
            kind: StepKind::ApplyPatch,
            detail: Some("apply patch".to_string()),
        },
        Step {
            kind: StepKind::ValidatePatch,
            detail: Some("validate patch".to_string()),
        },
    ]);
    spec.steps[0]
        .primitive
        .as_mut()
        .unwrap()
        .payload
        .as_object_mut()
        .unwrap()
        .insert("workspace".to_string(), json!(ws.to_str().unwrap()));

    let task_id = unique("p3-noev");
    let db = create_task(&task_id, &serde_json::to_string(&spec).unwrap(), "fix it");
    seed_patch(&db, &task_id, calc.to_str().unwrap());

    let err = execute_effects(&db, &task_id)
        .expect_err("completion without test evidence must be impossible");
    let msg = err.to_string();
    assert!(
        msg.contains("validation gate failed") || msg.contains("completion blocked"),
        "got: {msg}"
    );

    // Task state must not be completed.
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db).unwrap();
    let completed_steps: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM step_status WHERE task_id = ?1 AND status = 'committed'",
            rusqlite::params![task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        completed_steps < 2,
        "not all steps may be committed without evidence"
    );

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn full_chain_completes_with_real_evidence() {
    // Matrix I/A: apply + REAL passing tests + deterministic gate =>
    // completed, with kernel-owned evidence artifacts.
    let ws = fresh_dir("p3_chain_ok");
    python_project(
        &ws,
        "from calc import multiply\nassert multiply(2, 3) == 6\n",
    );
    let calc = ws.join("calc.py");

    let task_id = unique("p3-chain-ok");
    let spec = full_chain_spec(ws.to_str().unwrap());
    let db = create_task(&task_id, &spec.to_string(), "fix multiply");
    seed_patch(&db, &task_id, calc.to_str().unwrap());

    execute_effects(&db, &task_id).expect("full chain must complete");

    assert_eq!(
        std::fs::read_to_string(&calc).unwrap(),
        "def multiply(a, b):\n    return a * b\n"
    );

    let bus = EventBus::new(&db).unwrap();
    let artifacts = bus.list_semantic_artifacts(&task_id, None).unwrap();
    let has_report = artifacts.iter().any(|row| {
        serde_json::from_str::<serde_json::Value>(&row.payload)
            .ok()
            .and_then(|p| p.get("test_report_v1").cloned())
            .map(|r| r["passed"] == true && r["exit_code"] == 0)
            .unwrap_or(false)
    });
    assert!(has_report, "passing test_report_v1 must be persisted");
    let has_gate = artifacts.iter().any(|row| {
        serde_json::from_str::<serde_json::Value>(&row.payload)
            .ok()
            .map(|p| p.get("gate").and_then(|g| g.as_str()) == Some("deterministic_evidence"))
            .unwrap_or(false)
    });
    assert!(has_gate, "deterministic gate artifact must be persisted");

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn failing_real_tests_block_completion() {
    // Matrix J/B: real tests fail after apply => rejected step, no
    // completion, truthful reason.
    let ws = fresh_dir("p3_chain_bad");
    python_project(
        &ws,
        "from calc import multiply\nassert multiply(2, 3) == 999\n",
    );
    let calc = ws.join("calc.py");

    let task_id = unique("p3-chain-bad");
    let spec = full_chain_spec(ws.to_str().unwrap());
    let db = create_task(&task_id, &spec.to_string(), "fix multiply");
    seed_patch(&db, &task_id, calc.to_str().unwrap());

    let err = execute_effects(&db, &task_id).expect_err("failing tests must block");
    assert!(err.to_string().contains("real tests failed"), "{err}");

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn failing_pytest_style_tests_block_completion() {
    // CD-1 regression (acceptance F3): a pytest-style test file whose
    // assertions fail after the patch MUST block completion. Bare
    // `python3 <file>` used to exit 0 here because nothing invoked the
    // def test_* functions, producing test_report_v1 passed=true and a
    // false validation PASS for objectively failing tests.
    let ws = fresh_dir("p3_ptf_bad");
    python_project(
        &ws,
        "from calc import multiply\n\n\ndef test_multiply():\n    assert multiply(2, 3) == 999\n",
    );
    let calc = ws.join("calc.py");

    let task_id = unique("p3-ptf-bad");
    let spec = full_chain_spec(ws.to_str().unwrap());
    let db = create_task(&task_id, &spec.to_string(), "fix multiply");
    seed_patch(&db, &task_id, calc.to_str().unwrap());

    let err = execute_effects(&db, &task_id).expect_err("failing pytest-style tests must block");
    assert!(err.to_string().contains("real tests failed"), "{err}");

    // Task must NOT be completed.
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db).unwrap();
    let completed_steps: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM step_status WHERE task_id = ?1 AND status = 'committed'",
            rusqlite::params![task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        completed_steps < 3,
        "no completion when pytest-style tests fail"
    );

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn pytest_style_full_chain_completes_with_real_evidence() {
    // Positive counterpart: pytest-style tests that genuinely pass after
    // the patch still complete the chain with kernel-owned evidence.
    let ws = fresh_dir("p3_ptf_ok");
    python_project(
        &ws,
        "from calc import multiply\n\n\ndef test_multiply():\n    assert multiply(2, 3) == 6\n",
    );
    let calc = ws.join("calc.py");

    let task_id = unique("p3-ptf-ok");
    let spec = full_chain_spec(ws.to_str().unwrap());
    let db = create_task(&task_id, &spec.to_string(), "fix multiply");
    seed_patch(&db, &task_id, calc.to_str().unwrap());

    execute_effects(&db, &task_id).expect("pytest-style passing chain must complete");

    assert_eq!(
        std::fs::read_to_string(&calc).unwrap(),
        "def multiply(a, b):\n    return a * b\n"
    );

    let bus = EventBus::new(&db).unwrap();
    let artifacts = bus.list_semantic_artifacts(&task_id, None).unwrap();
    let has_report = artifacts.iter().any(|row| {
        serde_json::from_str::<serde_json::Value>(&row.payload)
            .ok()
            .and_then(|p| p.get("test_report_v1").cloned())
            .map(|r| {
                r["passed"] == true && r["exit_code"] == 0 && r["command_id"] == "python_test_file"
            })
            .unwrap_or(false)
    });
    assert!(has_report, "passing pytest-style report must be persisted");

    cleanup_task_files(&task_id, &db);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn concurrent_codefix_workloads_do_not_cross_contaminate() {
    // Matrix O: two independent CodeFix chains in parallel threads, each
    // with its own workspace/db — evidence must not leak between them.
    let mut handles = vec![];
    for i in 0..2 {
        handles.push(std::thread::spawn(move || {
            let ws = fresh_dir(&format!("p3_conc_{i}"));
            python_project(
                &ws,
                "from calc import multiply\nassert multiply(2, 3) == 6\n",
            );
            let calc = ws.join("calc.py");
            let task_id = unique(&format!("p3-conc-{i}"));
            let spec = full_chain_spec(ws.to_str().unwrap());
            let db = create_task(&task_id, &spec.to_string(), "fix multiply");
            seed_patch(&db, &task_id, calc.to_str().unwrap());
            execute_effects(&db, &task_id).expect("concurrent chain completes");

            // Verify THIS task's evidence references THIS workspace only.
            let bus = EventBus::new(&db).unwrap();
            let artifacts = bus.list_semantic_artifacts(&task_id, None).unwrap();
            let report = artifacts
                .iter()
                .rev()
                .find_map(|row| {
                    serde_json::from_str::<serde_json::Value>(&row.payload)
                        .ok()
                        .and_then(|p| p.get("test_report_v1").cloned())
                })
                .expect("own test report");
            let canon = std::fs::canonicalize(&ws).unwrap();
            assert_eq!(
                report["workspace"].as_str().unwrap(),
                canon.to_str().unwrap(),
                "test report must reference own workspace only"
            );
            cleanup_task_files(&task_id, &db);
            let _ = std::fs::remove_dir_all(&ws);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn evidence_is_deterministic_for_identical_inputs() {
    // Determinism: identical workspace + patch produce identical canonical
    // evidence fields (command_id, exit_code, passed). Stdout text of the
    // child process is not asserted bit-for-bit (not architecturally
    // guaranteed).
    let mut reports = vec![];
    for _ in 0..2 {
        let ws = fresh_dir("p3_det");
        python_project(
            &ws,
            "from calc import multiply\nassert multiply(2, 3) == 6\n",
        );
        let report =
            deterministic_ai_kernel::tools::test_runner::run_tests(ws.to_str().unwrap(), 30)
                .expect("run");
        reports.push(report);
        let _ = std::fs::remove_dir_all(&ws);
    }
    assert_eq!(reports[0].command_id, reports[1].command_id);
    assert_eq!(reports[0].exit_code, reports[1].exit_code);
    assert_eq!(reports[0].passed, reports[1].passed);
    assert_eq!(reports[0].argv, reports[1].argv);
}

#[tokio::test]
async fn run_tests_v1_requires_confirmation() {
    let ws = fresh_dir("p3_gate");
    python_project(&ws, "assert True\n");
    let denied = deterministic_ai_kernel::tools::registry::execute_tool(
        "run_tests_v1",
        &json!({"timeout_secs": 5}),
        ws.to_str().unwrap(),
        false,
    )
    .await;
    assert!(!denied.success);
    assert!(
        denied
            .error
            .unwrap_or_default()
            .contains("authorization required"),
        "run_tests_v1 must be confirmation-gated"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn unavailable_test_command_fails_closed_via_effects() {
    // Matrix D through the executor: workspace with no allowlisted test
    // command => terminal failure, never success.
    let ws = fresh_dir("p3_nocmd");
    std::fs::write(ws.join("main.py"), "x = 1\n").unwrap();

    let spec = PrimitiveSpec {
        id: PrimitiveId("run-tests-nocmd".to_string()),
        kind: PrimitiveKind::Compute,
        payload: json!({
            "step_kind": "RunTests",
            "workspace": ws.to_str().unwrap(),
            "timeout_secs": 10
        }),
    };
    let err = PrimitiveExecutor::execute("p3-nocmd", &spec, "payload")
        .expect_err("unavailable command must fail");
    assert!(
        err.to_string().contains("no allowlisted test command")
            || err.to_string().contains("real test execution failed"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&ws);
}
