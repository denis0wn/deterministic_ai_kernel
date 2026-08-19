use crate::providers::storage::StorageProvider;
use anyhow::{anyhow, Result};

use crate::kernel_types::TaskState;
use crate::providers;
use crate::scheduler::{next_ready_step, schedule};
use crate::worker;

fn payload_for_task(task_id: &str) -> Result<String> {
    let path = format!("artifacts/pipeline_input.{}.txt", task_id);
    match providers::get_filesystem().read_to_string(&path) {
        Ok(payload) => Ok(payload),
        // Tasks created outside pipeline-run (submit-task, plan-task) have no
        // input artifact; the execution loop must still be able to run them.
        Err(_) => Ok(String::new()),
    }
}

fn step_slug(step_id: &str) -> &str {
    step_id
        .split_once('_')
        .map(|(_, rest)| rest)
        .unwrap_or(step_id)
}

fn default_worker_for_step(step_id: &str) -> &'static str {
    match step_slug(step_id) {
        "analyze_task" | "plan_execution" | "read_repository" | "locate_bug"
        | "answer_question" => "worker-planner",
        "execute_changes" | "patch_code" | "apply_patch" | "run_tests" => "worker-executor",
        "validate_patch" | "validate_planner_output" => "worker-verifier",
        _ => "worker-planner",
    }
}

/// Find the most recent validated patch_v1 artifact produced by this task
/// (written by the PatchCode step as part of its primitive_result_v1
/// output). P2: this is how the kernel — not the LLM — hands the patch to
/// the ApplyPatch step.
fn find_latest_patch_v1(db: &str, task_id: &str) -> Result<Option<serde_json::Value>> {
    let bus = crate::event_bus::EventBus::new(db)?;
    let artifacts = bus.list_semantic_artifacts(task_id, None)?;
    for row in artifacts.iter().rev() {
        if row.artifact_type != "primitive_result_v1" {
            continue;
        }
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&row.payload) {
            if let Some(patch) = payload.get("patch_v1") {
                if patch.is_object() {
                    return Ok(Some(patch.clone()));
                }
            }
        }
    }
    Ok(None)
}

/// P3: find the most recent verified patch-apply evidence (output of the
/// apply_patch_v1 tool: {tool, status:"applied", evidence:{...}}).
fn find_latest_apply_evidence(db: &str, task_id: &str) -> Result<Option<serde_json::Value>> {
    let bus = crate::event_bus::EventBus::new(db)?;
    let artifacts = bus.list_semantic_artifacts(task_id, None)?;
    for row in artifacts.iter().rev() {
        if row.artifact_type != "primitive_result_v1" {
            continue;
        }
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&row.payload) {
            if payload.get("tool").and_then(|v| v.as_str()) == Some("apply_patch_v1")
                && payload.get("status").and_then(|v| v.as_str()) == Some("applied")
                && payload
                    .get("evidence")
                    .map(|e| e.is_object())
                    .unwrap_or(false)
            {
                return Ok(Some(payload));
            }
        }
    }
    Ok(None)
}

/// P3: find the most recent kernel-owned test_report_v1 (produced by REAL
/// RunTests execution — never by the LLM).
fn find_latest_test_report(db: &str, task_id: &str) -> Result<Option<serde_json::Value>> {
    let bus = crate::event_bus::EventBus::new(db)?;
    let artifacts = bus.list_semantic_artifacts(task_id, None)?;
    for row in artifacts.iter().rev() {
        if row.artifact_type != "primitive_result_v1" {
            continue;
        }
        if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&row.payload) {
            if let Some(report) = payload.get("test_report_v1") {
                if report.is_object() {
                    return Ok(Some(report.clone()));
                }
            }
        }
    }
    Ok(None)
}

/// Stage 4 decomposition: true iff the canonical event log carries a
/// kernel-owned SUBTASK_OF registration for this task. Read straight
/// from the append-only log — neither model output nor payload content
/// can produce this status.
fn is_registered_subtask(db: &str, task_id: &str) -> bool {
    rusqlite::Connection::open(db)
        .ok()
        .and_then(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM event_log WHERE task_id = ?1 AND event_type = 'SUBTASK_OF'",
                [task_id],
                |r| r.get::<_, i64>(0),
            )
            .ok()
        })
        .map(|n| n > 0)
        .unwrap_or(false)
}

pub fn execute_effects(db: &str, task_id: &str) -> Result<()> {
    providers::get_filesystem().create_dir_all("artifacts")?;
    let payload = payload_for_task(task_id)?;

    let storage = providers::storage_for(db);
    let spec = storage.load_exec_spec(task_id)?;

    loop {
        schedule(db, task_id)?;

        let step_id = if let Some(step_id) = next_ready_step(db, task_id)? {
            step_id
        } else {
            let dispatched = storage.find_dispatched_step(task_id)?;
            match dispatched {
                Some(dispatched_step_id) => dispatched_step_id,
                None => break,
            }
        };

        let worker_id = default_worker_for_step(&step_id).to_string();

        worker::claim_worker(db, task_id, &worker_id)?;
        worker::start_step(db, task_id, &worker_id, &step_id)?;

        let mut executed_via_primitive = false;
        if let Some(step_spec) = spec.steps.iter().find(|s| s.step_id == step_id) {
            if let Some(prim) = step_spec.primitive.as_ref() {
                executed_via_primitive = true;

                // P2: the ApplyPatch step consumes the validated patch_v1
                // artifact produced by PatchCode. The kernel injects it from
                // the canonical artifact store — the LLM never hands data to
                // the apply step directly.
                //
                // P3: ValidatePatch receives the same kernel-owned evidence
                // (apply evidence + real test report) so its completion gate
                // is a deterministic decision over facts, never over model
                // claims.
                let enriched;
                let slug = step_slug(&step_id);
                let prim_ref = if slug == "apply_patch" {
                    let mut p = prim.clone();
                    let patch = find_latest_patch_v1(db, task_id)?.ok_or_else(|| {
                        anyhow!(
                            "fatal: apply_patch step has no validated patch_v1 artifact (PatchCode must run first)"
                        )
                    })?;
                    p.payload["patch_v1"] = patch;
                    enriched = p;
                    &enriched
                } else if slug == "validate_patch" {
                    let mut p = prim.clone();
                    if let Some(ev) = find_latest_apply_evidence(db, task_id)? {
                        p.payload["apply_evidence"] = ev;
                    }
                    if let Some(report) = find_latest_test_report(db, task_id)? {
                        p.payload["test_report_v1"] = report;
                    }
                    enriched = p;
                    &enriched
                } else {
                    prim
                };

                match crate::execution::primitive_executor::PrimitiveExecutor::execute(
                    task_id, prim_ref, &payload,
                ) {
                    Ok(result) => {
                        // If it produced a text output (from Compute/Write), save it to final_answer for CLI compatibility
                        if let Some(text) = result.output.get("result").and_then(|v| v.as_str()) {
                            let answer_path = format!("artifacts/final_answer.{}.txt", task_id);
                            providers::get_filesystem().write(&answer_path, text)?;
                        }

                        // Generic recording of returned artifacts
                        let generation = storage.latest_generation_for_task(task_id)?;
                        for artifact in result.artifacts {
                            storage.append_semantic_artifact(
                                task_id,
                                &step_id,
                                generation,
                                &artifact.artifact_type,
                                &artifact.payload,
                            )?;
                        }

                        // Record the raw primitive result as an evidence artifact
                        storage.append_semantic_artifact(
                            task_id,
                            &step_id,
                            generation,
                            "primitive_result_v1",
                            &result.output,
                        )?;
                    }
                    Err(e) => {
                        // Kernel-detected contract violations (e.g. malformed
                        // patches) carry an explicit "fatal:" prefix and must
                        // reach classify_failure_outcome unprefixed so they
                        // become TERMINAL failures — an invalid patch must
                        // never be retryable forever or pass as success
                        // (P1, H-1 fix). Infrastructure errors stay wrapped
                        // and therefore retryable (fail-safe default).
                        let msg = e.to_string();
                        let reason = if msg.starts_with("fatal:") {
                            msg
                        } else {
                            format!("primitive_execution_error: {e}")
                        };
                        let _ = worker::fail_step(db, task_id, &worker_id, &step_id, &reason);
                        return Err(anyhow!("step {} failed: {}", step_id, reason));
                    }
                }
            }
        }

        if !executed_via_primitive {
            // A dispatched step without a primitive specification is a spec
            // corruption, not a transient error: fail it terminally and
            // report the task as failed (never as success).
            let reason = format!(
                "fatal: step {} does not have a primitive specification",
                step_id
            );
            let _ = worker::fail_step(db, task_id, &worker_id, &step_id, &reason);
            return Err(anyhow!("{}", reason));
        }

        worker::complete_step(db, task_id, &worker_id, &step_id)?;
    }

    // "Nothing dispatchable" is NOT success. The task's terminal state is
    // derived from the canonical fold: only a fully committed task succeeds.
    let state = storage.task_state(task_id)?;
    match state {
        TaskState::Completed => {
            // P3 defense-in-depth completion gate: a CodeFix-shaped task
            // (any spec containing an apply_patch step) may only complete
            // when the canonical store holds BOTH the verified patch-apply
            // evidence and a passing real test_report_v1. The step-level
            // gates already enforce this; reaching this point without the
            // evidence would mean a spec/flow corruption and must never be
            // reported as success (matrix H: completion without test
            // evidence is impossible).
            let has_apply_step = spec
                .steps
                .iter()
                .any(|s| s.step_id.ends_with("apply_patch"));
            if has_apply_step {
                let applied = find_latest_apply_evidence(db, task_id)?.is_some();
                let tests_passed = find_latest_test_report(db, task_id)?
                    .and_then(|r| r.get("passed").and_then(|p| p.as_bool()))
                    .unwrap_or(false);
                // PROGRESS UNTIL VERIFIED stage 4 — decomposition lemma:
                // a task kernel-registered as SUBTASK_OF may complete with
                // verified apply evidence but WITHOUT a passing test
                // report: its semantic verification is delegated to the
                // composition carrier's task-level tests (subtask proofs
                // are lemmas; the carrier is the theorem). Registration
                // is kernel-owned (SUBTASK_OF event), never model- or
                // payload-derived. Unregistered tasks keep the full gate
                // — matrix H remains impossible for them.
                let is_subtask = is_registered_subtask(db, task_id);
                if !applied || (!tests_passed && !is_subtask) {
                    return Err(anyhow!(
                        "fatal: CodeFix completion blocked: apply_evidence={} passing_test_report={} subtask={} — no fake success",
                        applied,
                        tests_passed,
                        is_subtask
                    ));
                }
                if is_subtask && !tests_passed {
                    println!("NOTE    : SUBTASK (lemma) completion — patch applied and kernel-verified; semantic verification delegated to the composition carrier.");
                }
            }
            storage.process_effects_ledger(task_id)?;
            println!("TASK_STATE: completed");
            Ok(())
        }
        TaskState::Failed => Err(anyhow!(
            "task {} failed: at least one step is terminally rejected",
            task_id
        )),
        other => Err(anyhow!(
            "task {} incomplete (state {}): retryable work remains",
            task_id,
            other.as_str()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::default_worker_for_step;

    #[test]
    fn worker_assignment_matches_codefix_capabilities() {
        // Planner steps
        assert_eq!(
            default_worker_for_step("00_read_repository"),
            "worker-planner"
        );
        assert_eq!(default_worker_for_step("01_locate_bug"), "worker-planner");
        assert_eq!(default_worker_for_step("00_analyze_task"), "worker-planner");
        assert_eq!(
            default_worker_for_step("01_plan_execution"),
            "worker-planner"
        );

        // Executor steps
        assert_eq!(default_worker_for_step("02_patch_code"), "worker-executor");
        assert_eq!(default_worker_for_step("03_run_tests"), "worker-executor");
        assert_eq!(
            default_worker_for_step("02_execute_changes"),
            "worker-executor"
        );
        // Apply step (P2): kernel-only mutation effect, executor worker.
        assert_eq!(default_worker_for_step("03_apply_patch"), "worker-executor");

        // Verifier steps
        assert_eq!(
            default_worker_for_step("04_validate_patch"),
            "worker-verifier"
        );
        assert_eq!(
            default_worker_for_step("02_validate_planner_output"),
            "worker-verifier"
        );

        // Question steps (P0, H-2 fix)
        assert_eq!(
            default_worker_for_step("00_answer_question"),
            "worker-planner"
        );

        // Unknown falls back to planner
        assert_eq!(default_worker_for_step("99_unknown_step"), "worker-planner");
    }
}
