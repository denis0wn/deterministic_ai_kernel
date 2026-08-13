use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution_abi::primitives::PrimitiveKind;
use deterministic_ai_kernel::workflow::contract::TaskClass;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "deterministic_ai_kernel_{}_{}.db",
        test_name, nanos
    ))
}

#[test]
fn codefix_exec_spec_has_correct_step_kinds_and_artifacts() {
    let spec = TaskClass::CodeFix.to_exec_spec(None);

    // Verify step ordering. P2: ApplyPatch is the kernel-only mutation step
    // between PatchCode (generation) and RunTests (verification).
    assert_eq!(spec.steps.len(), 6);
    assert_eq!(spec.steps[0].step_id, "00_read_repository");
    assert_eq!(spec.steps[1].step_id, "01_locate_bug");
    assert_eq!(spec.steps[2].step_id, "02_patch_code");
    assert_eq!(spec.steps[3].step_id, "03_apply_patch");
    assert_eq!(spec.steps[4].step_id, "04_run_tests");
    assert_eq!(spec.steps[5].step_id, "05_validate_patch");

    // Verify artifact flow declarations
    assert!(spec.steps[0]
        .outputs
        .contains(&"repository_content".to_string()));
    assert!(spec.steps[1]
        .inputs
        .contains(&"repository_content".to_string()));
    assert!(spec.steps[1].outputs.contains(&"bug_location".to_string()));
    assert!(spec.steps[2].inputs.contains(&"bug_location".to_string()));
    assert!(spec.steps[2].outputs.contains(&"patch_diff".to_string()));
    assert!(spec.steps[3].inputs.contains(&"patch_v1".to_string()));
    assert!(spec.steps[3]
        .outputs
        .contains(&"patch_apply_evidence".to_string()));
    assert!(spec.steps[4].inputs.contains(&"patch_diff".to_string()));
    assert!(spec.steps[4].outputs.contains(&"test_results".to_string()));
    assert!(spec.steps[5].inputs.contains(&"test_results".to_string()));
    assert!(spec.steps[5]
        .outputs
        .contains(&"validation_verdict".to_string()));

    // Verify dependencies are linear (5 entries for steps 1-5)
    assert_eq!(spec.dependencies.len(), 5);
    assert_eq!(spec.dependencies[0].step_id, "01_locate_bug");
    assert_eq!(spec.dependencies[0].depends_on, vec!["00_read_repository"]);
    assert_eq!(spec.dependencies[1].step_id, "02_patch_code");
    assert_eq!(spec.dependencies[1].depends_on, vec!["01_locate_bug"]);
    assert_eq!(spec.dependencies[2].step_id, "03_apply_patch");
    assert_eq!(spec.dependencies[2].depends_on, vec!["02_patch_code"]);
    assert_eq!(spec.dependencies[3].step_id, "04_run_tests");
    assert_eq!(spec.dependencies[3].depends_on, vec!["03_apply_patch"]);
    assert_eq!(spec.dependencies[4].step_id, "05_validate_patch");
    assert_eq!(spec.dependencies[4].depends_on, vec!["04_run_tests"]);
}

#[test]
fn codefix_primitive_execution_end_to_end() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let db = unique_db_path("codefix_e2e");
        let _ = fs::remove_file(&db);

        let bus = EventBus::new(&db).expect("test failure");

        // Emit a pipeline.created event to establish task context
        bus.append_event(
            "codefix-e2e",
            None,
            "pipeline.created",
            &json!({"task_class": "CodeFix"}),
        )
        .expect("test failure");

        let spec = TaskClass::CodeFix.to_exec_spec(None);

        // Execute each step's primitive through PrimitiveExecutor.
        // Skip steps that require LLM (no mock available in this test).
        // Emit the full canonical lifecycle (lease -> dispatched -> started ->
        // completed) for every step so the canonical replay fold accepts the
        // stream (audit findings C1/R2).
        let mut executed = 0;
        for step in &spec.steps {
            if let Some(ref prim) = step.primitive {
                let requires_llm = prim
                    .payload
                    .get("requires_llm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let lease_id = format!("codefix-e2e/{}/lease/1", step.step_id);
                bus.append_event(
                    "codefix-e2e",
                    Some(&step.step_id),
                    "LEASE_ACQUIRED",
                    &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
                )
                .expect("test failure");
                bus.append_event(
                    "codefix-e2e",
                    Some(&step.step_id),
                    "STEP_DISPATCHED",
                    &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
                )
                .expect("test failure");
                bus.append_event(
                    "codefix-e2e",
                    Some(&step.step_id),
                    "STEP_STARTED",
                    &json!({"lease_id": lease_id, "worker_id": "worker-test"}),
                )
                .expect("test failure");

                if requires_llm {
                    bus.append_event(
                        "codefix-e2e",
                        Some(&step.step_id),
                        "STEP_COMPLETED",
                        &json!({"lease_id": lease_id, "step_id": step.step_id, "status": "skipped_no_llm"}),
                    )
                    .expect("test failure");
                    executed += 1;
                    continue;
                }

                // P2: the ApplyPatch step is a kernel-only effect that
                // requires the patch_v1 artifact injected by the effect loop
                // (execute_effects). This harness executes steps standalone,
                // so the apply step is recorded as skipped here; the real
                // apply path is covered by tests/patch_apply_p2.rs.
                //
                // P3: the RunTests step performs REAL authorized execution
                // and requires an authorized workspace + the effect loop; it
                // is skipped in this standalone harness and covered by
                // tests/patch_apply_p2.rs (full chain) and the live model
                // acceptance. The ValidatePatch step is a deterministic
                // evidence gate that likewise needs the effect loop's
                // injected artifacts, so it is skipped here too.
                if step.step_id.ends_with("apply_patch")
                    || step.step_id.ends_with("run_tests")
                    || step.step_id.ends_with("validate_patch")
                {
                    bus.append_event(
                        "codefix-e2e",
                        Some(&step.step_id),
                        "STEP_COMPLETED",
                        &json!({"lease_id": lease_id, "step_id": step.step_id, "status": "skipped_no_effect_loop"}),
                    )
                    .expect("test failure");
                    executed += 1;
                    continue;
                }

                let result = PrimitiveExecutor::execute("codefix-e2e", prim, "test payload");
                match result {
                    Ok(r) => {
                        bus.append_event(
                            "codefix-e2e",
                            Some(&step.step_id),
                            "STEP_COMPLETED",
                            &json!({"lease_id": lease_id, "step_id": step.step_id, "status": r.status}),
                        )
                        .expect("test failure");
                        executed += 1;
                    }
                    Err(e) => {
                        panic!("Step {} failed: {}", step.step_id, e);
                    }
                }
            }
        }

        // Verify all step events were recorded:
        // 1 pipeline.created + 4 lifecycle events per executed step.
        let events = bus
            .list_execution_events("codefix-e2e")
            .expect("test failure");
        assert_eq!(events.len(), 1 + executed * 4);

        // Verify replay validation passes
        let db_str = db.to_str().unwrap();
        assert!(deterministic_ai_kernel::replay::engine::replay_validate(
            db_str,
            "codefix-e2e"
        ));

        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(format!("{}-wal", db.display()));
        let _ = fs::remove_file(format!("{}-shm", db.display()));
    });
}

#[test]
fn codefix_step_kind_to_primitive_mapping() {
    let spec = TaskClass::CodeFix.to_exec_spec(None);

    let expected_primitives = [
        (PrimitiveKind::Read, "read_repository"),
        (PrimitiveKind::Compute, "locate_bug"),
        (PrimitiveKind::Write, "patch_code"),
        (PrimitiveKind::Compute, "apply_patch"),
        (PrimitiveKind::Compute, "run_tests"),
        (PrimitiveKind::Route, "validate_patch"),
    ];

    for (step, (expected_kind, name)) in spec.steps.iter().zip(expected_primitives.iter()) {
        let prim = step
            .primitive
            .as_ref()
            .unwrap_or_else(|| panic!("{} should have a primitive", name));
        assert_eq!(
            prim.kind, *expected_kind,
            "{} should map to {:?}",
            name, expected_kind
        );
    }
}
