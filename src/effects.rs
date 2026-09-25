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

pub(crate) fn step_slug(step_id: &str) -> &str {
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
pub(crate) fn find_latest_patch_v1(db: &str, task_id: &str) -> Result<Option<serde_json::Value>> {
    let bus = crate::event_bus::EventBus::new(db)?;
    let artifacts = bus.list_semantic_artifacts(task_id, None)?;
    for row in artifacts.iter() { // list is DESC (newest first): first match = LATEST. (.rev() used to return the OLDEST — a latent stale-read exposed by the feedback loop, which writes multiple reports per task.)
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

/// G1 STRICT LINEAGE member validation (design: G1_CARRYOVER_DESIGN_
/// REVIEW.md §A4; owner decision: STRICT LINEAGE, fail-closed). Every
/// member MUST, else fatal: (a) exist in tasks; (b) be CodeFix; (c)
/// carry a validated patch_v1 artifact; (d) target the composition's
/// target_file; (e) carry provenance (task_id match + source_generation).
fn validate_member_lineage(
    db: &str,
    member: &str,
    composition: &crate::exec_spec::CompositionSpec,
) -> Result<()> {
    let conn = rusqlite::Connection::open(db)
        .map_err(|e| anyhow!("fatal: lineage check cannot open db: {e}"))?;
    // (a) existence + (b) task class
    let (count, class): (i64, String) = conn
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(task_class),'') FROM tasks WHERE task_id = ?1",
            [member],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| anyhow!("fatal: lineage query failed for member {member}: {e}"))?;
    if count == 0 {
        return Err(anyhow!(
            "fatal: composition member {member} does not exist (strict lineage)"
        ));
    }
    if class != "CodeFix" {
        return Err(anyhow!(
            "fatal: composition member {member} is not a CodeFix task (strict lineage)"
        ));
    }
    // (c) validated patch artifact + (d) target match + (e) provenance
    let patch_value = find_latest_patch_v1(db, member)?.ok_or_else(|| {
        anyhow!("fatal: composition member {member} has no validated patch_v1 artifact (strict lineage)")
    })?;
    let target = patch_value
        .get("target_file")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if target != composition.target_file {
        return Err(anyhow!(
            "fatal: composition member {member} targets '{target}' but composition targets '{}' (strict lineage)",
            composition.target_file
        ));
    }
    // provenance: the artifact row must belong to the member task with a
    // source_generation (checked via the artifact query in
    // find_latest_patch_v1 which is task-scoped) — additionally require a
    // source_generation to be present on the member's artifact row.
    let has_provenance: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM semantic_artifacts WHERE task_id = ?1 AND artifact_type = 'primitive_result_v1' AND payload LIKE '%\"patch_v1\"%' AND source_generation IS NOT NULL",
            [member],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if has_provenance == 0 {
        return Err(anyhow!(
            "fatal: composition member {member} patch artifact lacks provenance (strict lineage)"
        ));
    }
    Ok(())
}

/// G1 carried-patch composition (design: G1_CARRYOVER_DESIGN_REVIEW.md).
/// Rebuilds the composition target from the anchored pristine baseline by
/// re-applying the ORDERED, strict-lineage-validated member patches —
/// zero model involvement. Fail-closed on any ambiguity. Records a
/// combined apply-evidence artifact so the deterministic completion gate
/// (apply evidence + passing tests) sees a single applied state.
fn apply_composed_patches(
    db: &str,
    task_id: &str,
    composition: &crate::exec_spec::CompositionSpec,
    storage: &crate::providers::storage::DefaultStorage,
) -> Result<()> {
    use crate::providers::storage::StorageProvider;
    // Workspace authorization: same source as the apply primitive.
    let workspace = std::env::var("DAK_CODEFIX_WORKSPACE").map_err(|_| {
        anyhow!("fatal: composition apply has no authorized workspace (set DAK_CODEFIX_WORKSPACE)")
    })?;
    // A3 baseline anchor: current target must equal the anchored baseline
    // hash (BLAKE3). Mismatch ⇒ refuse (never trust unanchored bytes).
    let target_path = crate::tools::file_tools::resolve_safe(&composition.target_file, &workspace)
        .map_err(|e| anyhow!("fatal: composition target not confined to workspace: {e}"))?;
    let current = std::fs::read_to_string(&target_path)
        .map_err(|e| anyhow!("fatal: composition target unreadable: {e}"))?;
    let current_hash = blake3::hash(current.as_bytes()).to_hex().to_string();
    if current_hash != composition.baseline_hash {
        return Err(anyhow!(
            "fatal: composition baseline mismatch — refusing to apply on unanchored state (target {} hash {} != baseline {})",
            composition.target_file, current_hash, composition.baseline_hash
        ));
    }
    // A2: ordered members; zero members ⇒ refuse (no fabricated success).
    if composition.members.is_empty() {
        return Err(anyhow!(
            "fatal: composition has no members (no fabricated success)"
        ));
    }
    let mut member_evidence: Vec<serde_json::Value> = Vec::new();
    let mut last_evidence: Option<crate::execution::patch_apply::PatchApplyEvidence> = None;
    for member in &composition.members {
        validate_member_lineage(db, member, composition)?;
        let patch_value = find_latest_patch_v1(db, member)?.ok_or_else(|| {
            anyhow!("fatal: composition member {member} has no validated patch_v1 artifact")
        })?;
        let patch: crate::execution::patch_contract::PatchV1 = serde_json::from_value(patch_value)
            .map_err(|e| anyhow!("fatal: member {member} patch schema violation: {e}"))?;
        // A5: apply each member patch IN ORDER via the existing machinery;
        // each is re-grounded against the CURRENT composed content (overlap/
        // no-op/stale/order all fail-closed there).
        let evidence = crate::execution::patch_apply::apply_patch_v1(&patch, &workspace)
            .map_err(|e| anyhow!("fatal: composition member {member} apply failed: {e}"))?;
        member_evidence.push(serde_json::json!({
            "member_task_id": member,
            "pre_image_blake3": evidence.pre_image_blake3,
            "post_image_blake3": evidence.post_image_blake3,
            "applied": true,
        }));
        last_evidence = Some(evidence);
    }
    let last =
        last_evidence.ok_or_else(|| anyhow!("fatal: composition produced no apply evidence"))?;

    // Composed-state hash, measured independently from the bytes actually on
    // disk after every member applied. Copying `last.post_image_blake3` here
    // would make the analyzer's `composed_state` link tautological; measuring
    // it keeps the link meaningful — a disagreement between what apply_patch
    // reported and what the file really contains now surfaces as a chain
    // inconsistency instead of verifying by construction.
    //
    // This is the field analyzer::evidence_chain_v2::parse_composition_inputs
    // requires. Without it the v2 verifier refuses real kernel output with
    // "field 'composed_state_blake3' missing": the producer lived on
    // orchestrator-rebuild and the verifier on analyzer, and nothing exercised
    // them together until the two branches were merged.
    let composed_path =
        crate::tools::file_tools::resolve_safe(&composition.target_file, &workspace).map_err(
            |e| {
                anyhow!(
                    "fatal: composition target '{}' rejected by workspace confinement: {e}",
                    composition.target_file
                )
            },
        )?;
    let composed_bytes = std::fs::read(&composed_path).map_err(|e| {
        anyhow!(
            "fatal: cannot read composed state '{}': {e}",
            composed_path.display()
        )
    })?;
    let composed_state_blake3 = blake3::hash(&composed_bytes).to_hex().to_string();

    // Combined apply-evidence artifact (same shape the apply tool emits so
    // the deterministic completion gate recognizes it).
    let combined = serde_json::json!({
        "tool": "apply_patch_v1",
        "status": "applied",
        "evidence": last,
        "composition_members": member_evidence,
        "composition_baseline_hash": composition.baseline_hash,
        "composition_target_file": composition.target_file,
        "composed_state_blake3": composed_state_blake3,
    });
    let generation = storage.latest_generation_for_task(task_id)?;
    storage.append_semantic_artifact(
        task_id,
        "03_apply_patch",
        generation,
        "primitive_result_v1",
        &combined,
    )?;
    Ok(())
}

/// P3: find the most recent verified patch-apply evidence (output of the
/// apply_patch_v1 tool: {tool, status:"applied", evidence:{...}}).
fn find_latest_apply_evidence(db: &str, task_id: &str) -> Result<Option<serde_json::Value>> {
    let bus = crate::event_bus::EventBus::new(db)?;
    let artifacts = bus.list_semantic_artifacts(task_id, None)?;
    for row in artifacts.iter() { // list is DESC (newest first): first match = LATEST. (.rev() used to return the OLDEST — a latent stale-read exposed by the feedback loop, which writes multiple reports per task.)
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
    for row in artifacts.iter() { // list is DESC (newest first): first match = LATEST. (.rev() used to return the OLDEST — a latent stale-read exposed by the feedback loop, which writes multiple reports per task.)
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
                // G1: a carried-patch composition applies its ORDERED,
                // strict-lineage-validated member patches directly (zero
                // model involvement) and records a combined apply-evidence
                // artifact; the generic single-patch primitive path is
                // skipped for this step.
                let mut composition_handled = false;
                let prim_ref = if slug == "apply_patch" {
                    if let Some(composition) = spec.composition.as_ref() {
                        apply_composed_patches(db, task_id, composition, &storage)?;
                        composition_handled = true;
                        let mut p = prim.clone();
                        p.payload["g1_composition"] = serde_json::json!({
                            "members": composition.members,
                            "baseline_hash": composition.baseline_hash,
                            "target_file": composition.target_file,
                        });
                        enriched = p;
                        &enriched
                    } else {
                        let mut p = prim.clone();
                        let patch = find_latest_patch_v1(db, task_id)?.ok_or_else(|| {
                            anyhow!(
                                "fatal: apply_patch step has no validated patch_v1 artifact (PatchCode must run first)"
                            )
                        })?;
                        p.payload["patch_v1"] = patch;
                        enriched = p;
                        &enriched
                    }
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

                if composition_handled {
                    // Composition already applied + recorded evidence; the
                    // step completes via complete_step below.
                } else {
                    match crate::execution::primitive_executor::PrimitiveExecutor::execute(
                        task_id, prim_ref, &payload,
                    ) {
                        Ok(result) => {
                            // If it produced a text output (from Compute/Write), save it to final_answer for CLI compatibility
                            if let Some(text) = result.output.get("result").and_then(|v| v.as_str())
                            {
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
                            // C1: a failed RunTests step carries its
                            // TestReportV1 — persist it as an artifact
                            // before the task dies. Failing reports are
                            // the feedback loop's input; without this the
                            // report survived only inside the error string.
                            // Same type and shape as the success path's
                            // step output (semantic_artifacts has a CHECK
                            // constraint on artifact_type).
                            if let Some(trf) = e.downcast_ref::<crate::execution::primitive_executor::TestRunFailure>()
                            {
                                let generation =
                                    storage.latest_generation_for_task(task_id)?;
                                storage.append_semantic_artifact(
                                    task_id,
                                    &step_id,
                                    generation,
                                    "primitive_result_v1",
                                    &serde_json::json!({
                                        "test_report_v1": trf.report,
                                        "tests_passed": false,
                                        "step_id": step_id,
                                    }),
                                )?;

                                // Layer-2 POC (spec §5, security review
                                // 2026-09-25 C1–C4): a tests_failed
                                // run_tests step with located failing-test
                                // names opens a bounded feedback cycle that
                                // re-enters this same executor path.
                                if slug == "run_tests"
                                    && trf
                                        .report
                                        .get("classification")
                                        .and_then(|v| v.as_str())
                                        == Some(crate::tools::test_runner::outcome::TESTS_FAILED)
                                {
                                    // NotEligible / Exhausted / loop error
                                    // all fall through to the honest
                                    // terminal failure below.
                                    if let Ok(
                                        crate::execution::feedback::LoopOutcome::Converted { .. },
                                    ) = crate::execution::feedback::maybe_run(
                                        db,
                                        task_id,
                                        &spec,
                                        &payload,
                                        &trf.report,
                                    ) {
                                        worker::complete_step(
                                            db, task_id, &worker_id, &step_id,
                                        )?;
                                        continue;
                                    }
                                }
                            }
                            // C2-gap fix: a terminally malformed patch
                            // persists its model calls (the corrupted raw
                            // response is the evidence base for repair
                            // patterns).
                            if let Some(pf) = e.downcast_ref::<crate::execution::primitive_executor::PatchFailure>()
                            {
                                let generation =
                                    storage.latest_generation_for_task(task_id)?;
                                storage.append_semantic_artifact(
                                    task_id,
                                    &step_id,
                                    generation,
                                    "primitive_result_v1",
                                    &serde_json::json!({
                                        "step_id": step_id,
                                        "patch_failed": true,
                                        "llm_calls": pf.llm_calls,
                                    }),
                                )?;
                            }
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
                } // end else (generic primitive execution)
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
