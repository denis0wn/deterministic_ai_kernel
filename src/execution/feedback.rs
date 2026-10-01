//! C3: feedback-cycle event vocabulary (Layer 2 prerequisite).
//!
//! The loop itself lands with the Layer-2 POC (`LAYER2_VERIFIER_FEEDBACK_SPEC.md`);
//! these event types are the contract it will emit through the canonical event
//! bus so replay and the analyzer see every cycle. Locked now so the POC diff
//! is mechanics, not vocabulary. All payloads are kernel-derived only: failing
//! test names, hashes, indices — never model text, never test output.

/// A `tests_failed` step with known failing-test names opened a cycle.
/// Payload: `{failing_tests: [name], budget: u64}`.
pub const FEEDBACK_CYCLE_STARTED: &str = "FEEDBACK_CYCLE_STARTED";

/// One feedback attempt re-entered patch generation.
/// Payload: `{attempt: u64, prior_patch_blake3: String, failing_tests: [name]}`.
pub const FEEDBACK_ATTEMPT: &str = "FEEDBACK_ATTEMPT";

/// Budget exhausted or the model repeated an identical patch (temp-0
/// futility) — the task ends in the same honest failure as without a loop.
/// Payload: `{attempts: u64, reason: "budget_exhausted" | "identical_patch"}`.
pub const FEEDBACK_EXHAUSTED: &str = "FEEDBACK_EXHAUSTED";

/// An attempt's tests passed — the cycle converted the failure. The task
/// proceeds to validation unchanged; success is never widened.
/// Payload: `{attempt: u64}`.
pub const FEEDBACK_CONVERTED: &str = "FEEDBACK_CONVERTED";

use anyhow::Result;
use serde_json::Value;

/// POC budget (LAYER2_VERIFIER_FEEDBACK_SPEC §5.2): hard-coded, not
/// configurable. Two feedback attempts = three patch attempts total.
pub const MAX_FEEDBACK_ATTEMPTS: u32 = 2;

/// Kill switch: DAK_FEEDBACK_LOOP=off|0|false disables the loop; tasks then
/// fail on the first bad patch exactly as before. Default: enabled.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("DAK_FEEDBACK_LOOP").ok().as_deref(),
        Some("off") | Some("0") | Some("false")
    )
}

pub enum LoopOutcome {
    /// Tests passed on the given feedback attempt.
    Converted { attempts: u32 },
    /// Honest failure: budget exhausted, identical patch, or infra error.
    Exhausted { attempts: u32, reason: String },
    /// Preconditions not met; the task fails exactly as without the loop.
    NotEligible { reason: &'static str },
}

/// Feedback block appended to the patch prompt on loop attempts. Content:
/// attempt index and failing test NAMES (C0-validated identifiers) — never
/// test output, never expected values (spec §2: located rung only).
pub fn augment_patch_prompt(prompt: String, feedback: &Value) -> String {
    let attempt = feedback
        .get("attempt")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    let names: Vec<&str> = feedback
        .get("failing_tests")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let names = if names.is_empty() {
        "(not located)".to_string()
    } else {
        names.join(", ")
    };
    format!(
        "{prompt}\n\nVERIFICATION FEEDBACK (kernel-authoritative):\nAttempt {attempt}: your previous patch was applied and the REAL test suite FAILED.\nFailing tests: {names}\nThe task contract is still unmet. Produce a DIFFERENT fix that makes the named tests pass; do not repeat the previous approach."
    )
}

/// Patch identity for the futility stop: only the semantic triple counts
/// (target, anchors, replacement). Model prose like `reason` varies between
/// attempts and must not defeat the stop — measured 2026-09-26: Ministral
/// emitted the identical replacement with a different `reason` three times
/// and the raw-JSON hash let it through twice.
fn patch_semantic_hash(patch: &Value) -> Option<String> {
    let p = patch.get("patch_v1").unwrap_or(patch);
    let (t, c, r) = (
        p.get("target_file")?.as_str()?,
        p.get("context_before")?.as_str()?,
        p.get("replacement")?.as_str()?,
    );
    Some(
        blake3::hash(format!("{t}\x00{c}\x00{r}").as_bytes())
            .to_hex()
            .to_string(),
    )
}

fn failing_test_names(report: &Value) -> Vec<String> {
    report
        .get("failures")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The read step's artifact carries the pre-patch file image
/// (`{"path", "content"}`) — the rollback source for each attempt.
fn read_pre_image(db: &str, task_id: &str) -> Result<Option<(String, String)>> {
    let bus = crate::event_bus::EventBus::new(db)?;
    for row in bus.list_semantic_artifacts(task_id, None)?.iter().rev() {
        if !row.step_id.contains("read_repository") {
            continue;
        }
        let Ok(p) = serde_json::from_str::<Value>(&row.payload) else {
            continue;
        };
        let (Some(path), Some(content)) = (
            p.get("path").and_then(|v| v.as_str()),
            p.get("content").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        return Ok(Some((path.to_string(), content.to_string())));
    }
    Ok(None)
}

/// Roll the patch target back to its pre-image so every attempt patches the
/// SAME starting state (identical-patch detection is only meaningful this
/// way). The path comes from a kernel artifact and is re-confined anyway;
/// the write goes through the same filesystem provider the executor uses.
fn restore_pre_image(read_path: &str, content: &str, patch_prim_payload: &Value) -> Result<()> {
    let workspace = crate::execution::primitive_executor::authorized_workspace(patch_prim_payload);
    let canonical =
        crate::execution::primitive_executor::confined(read_path, workspace.as_deref())?;
    crate::providers::get_filesystem().write(&canonical.to_string_lossy(), content)?;
    Ok(())
}

fn prim_for(
    spec: &crate::exec_spec::ExecSpec,
    slug_want: &str,
) -> Option<(String, crate::execution_abi::primitives::PrimitiveSpec)> {
    spec.steps
        .iter()
        .find(|s| crate::effects::step_slug(&s.step_id) == slug_want)
        .and_then(|s| s.primitive.clone().map(|p| (s.step_id.clone(), p)))
}

/// The bounded verifier-driven feedback loop (LAYER2_VERIFIER_FEEDBACK_SPEC
/// §5.2). Called from the effects loop when a run_tests step failed with
/// `tests_failed`. Re-enters the CANONICAL executor path (same
/// PrimitiveExecutor, same confinement, same artifact persistence) — only
/// the orchestration differs, because the plan graph has no cycles.
/// Security review conditions C1/C3: executor reuse; `failures[]` is
/// advisory prompt content, never pass/fail evidence (that stays with
/// exit code + harness OK marker).
pub fn maybe_run(
    db: &str,
    task_id: &str,
    spec: &crate::exec_spec::ExecSpec,
    task_payload: &str,
    first_report: &Value,
) -> Result<LoopOutcome> {
    use crate::execution::primitive_executor::{PrimitiveExecutor, TestRunFailure};

    if !enabled() {
        return Ok(LoopOutcome::NotEligible {
            reason: "disabled via DAK_FEEDBACK_LOOP",
        });
    }
    let mut failing_tests = failing_test_names(first_report);
    if failing_tests.is_empty() {
        return Ok(LoopOutcome::NotEligible {
            reason: "no located failing test",
        });
    }
    let Some((patch_step_id, patch_prim)) = prim_for(spec, "patch_code") else {
        return Ok(LoopOutcome::NotEligible {
            reason: "no patch_code step",
        });
    };
    let Some((apply_step_id, apply_prim)) = prim_for(spec, "apply_patch") else {
        return Ok(LoopOutcome::NotEligible {
            reason: "no apply_patch step",
        });
    };
    let Some((tests_step_id, tests_prim)) = prim_for(spec, "run_tests") else {
        return Ok(LoopOutcome::NotEligible {
            reason: "no run_tests step",
        });
    };
    let Some((read_path, pre_image)) = read_pre_image(db, task_id)? else {
        return Ok(LoopOutcome::NotEligible {
            reason: "no read pre-image to roll back to",
        });
    };

    let storage = crate::providers::storage_for(db);
    use crate::providers::storage::StorageProvider;
    let bus = crate::event_bus::EventBus::new(db)?;
    bus.append_event(
        task_id,
        Some(&tests_step_id),
        FEEDBACK_CYCLE_STARTED,
        &serde_json::json!({"failing_tests": failing_tests, "budget": MAX_FEEDBACK_ATTEMPTS}),
    )?;

    let mut prior_hash =
        crate::effects::find_latest_patch_v1(db, task_id)?.and_then(|p| patch_semantic_hash(&p));
    let mut attempts = 0u32;

    loop {
        if attempts >= MAX_FEEDBACK_ATTEMPTS {
            bus.append_event(
                task_id,
                Some(&tests_step_id),
                FEEDBACK_EXHAUSTED,
                &serde_json::json!({"attempts": attempts, "reason": "budget_exhausted"}),
            )?;
            return Ok(LoopOutcome::Exhausted {
                attempts,
                reason: "budget_exhausted".to_string(),
            });
        }
        attempts += 1;

        restore_pre_image(&read_path, &pre_image, &patch_prim.payload)?;

        bus.append_event(
            task_id,
            Some(&patch_step_id),
            FEEDBACK_ATTEMPT,
            &serde_json::json!({
                "attempt": attempts,
                "prior_patch_blake3": prior_hash,
                "failing_tests": failing_tests,
            }),
        )?;

        // patch attempt (canonical executor, feedback-augmented payload)
        let mut prim = patch_prim.clone();
        prim.payload["feedback"] = serde_json::json!({
            "attempt": attempts,
            "failing_tests": failing_tests,
            "prior_patch_blake3": prior_hash,
        });
        let result = match PrimitiveExecutor::execute(task_id, &prim, task_payload) {
            Ok(r) => r,
            Err(e) => {
                bus.append_event(
                    task_id,
                    Some(&tests_step_id),
                    FEEDBACK_EXHAUSTED,
                    &serde_json::json!({"attempts": attempts, "reason": "patch_error"}),
                )?;
                return Ok(LoopOutcome::Exhausted {
                    attempts,
                    reason: format!("patch_error: {e}"),
                });
            }
        };
        let mut out = result.output.clone();
        out["feedback_attempt"] = serde_json::json!(attempts);
        let generation = storage.latest_generation_for_task(task_id)?;
        storage.append_semantic_artifact(
            task_id,
            &patch_step_id,
            generation,
            "primitive_result_v1",
            &out,
        )?;

        // identical patch => at temperature 0 the model cannot use the
        // signal; further attempts are futile (spec §5.2).
        let new_hash = out.get("patch_v1").and_then(patch_semantic_hash);
        if new_hash.is_some() && new_hash == prior_hash {
            bus.append_event(
                task_id,
                Some(&tests_step_id),
                FEEDBACK_EXHAUSTED,
                &serde_json::json!({"attempts": attempts, "reason": "identical_patch"}),
            )?;
            return Ok(LoopOutcome::Exhausted {
                attempts,
                reason: "identical_patch".to_string(),
            });
        }
        prior_hash = new_hash;

        // apply attempt (same injection the chain uses)
        let mut aprim = apply_prim.clone();
        aprim.payload["patch_v1"] = out["patch_v1"].clone();
        let ares = match PrimitiveExecutor::execute(task_id, &aprim, task_payload) {
            Ok(r) => r,
            Err(e) => {
                bus.append_event(
                    task_id,
                    Some(&tests_step_id),
                    FEEDBACK_EXHAUSTED,
                    &serde_json::json!({"attempts": attempts, "reason": "apply_error"}),
                )?;
                return Ok(LoopOutcome::Exhausted {
                    attempts,
                    reason: format!("apply_error: {e}"),
                });
            }
        };
        let mut aout = ares.output.clone();
        aout["feedback_attempt"] = serde_json::json!(attempts);
        let generation = storage.latest_generation_for_task(task_id)?;
        storage.append_semantic_artifact(
            task_id,
            &apply_step_id,
            generation,
            "primitive_result_v1",
            &aout,
        )?;

        // verification attempt
        match PrimitiveExecutor::execute(task_id, &tests_prim, task_payload) {
            Ok(r) => {
                let mut tout = r.output.clone();
                tout["feedback_attempt"] = serde_json::json!(attempts);
                let generation = storage.latest_generation_for_task(task_id)?;
                storage.append_semantic_artifact(
                    task_id,
                    &tests_step_id,
                    generation,
                    "primitive_result_v1",
                    &tout,
                )?;
                bus.append_event(
                    task_id,
                    Some(&tests_step_id),
                    FEEDBACK_CONVERTED,
                    &serde_json::json!({"attempt": attempts}),
                )?;
                return Ok(LoopOutcome::Converted { attempts });
            }
            Err(e) => {
                if let Some(trf) = e.downcast_ref::<TestRunFailure>() {
                    let tout = serde_json::json!({
                        "test_report_v1": trf.report,
                        "tests_passed": false,
                        "feedback_attempt": attempts,
                    });
                    let generation = storage.latest_generation_for_task(task_id)?;
                    storage.append_semantic_artifact(
                        task_id,
                        &tests_step_id,
                        generation,
                        "primitive_result_v1",
                        &tout,
                    )?;
                    let cls = trf
                        .report
                        .get("classification")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    if cls != crate::tools::test_runner::outcome::TESTS_FAILED {
                        // timeout / infrastructure — not a semantic failure,
                        // feedback cannot help
                        bus.append_event(
                            task_id,
                            Some(&tests_step_id),
                            FEEDBACK_EXHAUSTED,
                            &serde_json::json!({"attempts": attempts, "reason": format!("infrastructure:{cls}")}),
                        )?;
                        return Ok(LoopOutcome::Exhausted {
                            attempts,
                            reason: format!("infrastructure: {cls}"),
                        });
                    }
                    let new_names = failing_test_names(&trf.report);
                    if !new_names.is_empty() {
                        failing_tests = new_names;
                    }
                    continue;
                }
                bus.append_event(
                    task_id,
                    Some(&tests_step_id),
                    FEEDBACK_EXHAUSTED,
                    &serde_json::json!({"attempts": attempts, "reason": "step_error"}),
                )?;
                return Ok(LoopOutcome::Exhausted {
                    attempts,
                    reason: format!("step_error: {e}"),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventBus;

    /// The canonical bus accepts, persists and returns the cycle vocabulary —
    /// the channel the POC will emit through.
    #[test]
    fn feedback_events_roundtrip_through_event_bus() {
        let db = std::env::temp_dir().join(format!(
            "dak_c3_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db_str = db.to_string_lossy().into_owned();
        let bus = EventBus::new(&db_str).unwrap();
        bus.append_event(
            "c3-task",
            Some("04_run_tests"),
            FEEDBACK_CYCLE_STARTED,
            &serde_json::json!({"failing_tests": ["test_a"], "budget": 2}),
        )
        .unwrap();
        let events = bus.query("c3-task").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, FEEDBACK_CYCLE_STARTED);
        assert!(events[0].payload.contains("test_a"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(format!("{db_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_str}-shm"));
    }
}
