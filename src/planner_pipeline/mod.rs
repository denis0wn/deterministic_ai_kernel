use crate::semantic_bias::BiasVersion;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub fn get_environment_fingerprint() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let rustc_version = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    let kernel_version = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "unknown_kernel".to_string());

    let mlx_model_dir = std::env::var("MLX_MODEL_DIR").unwrap_or_default();
    let cargo_pkg_name = std::env::var("CARGO_PKG_NAME").unwrap_or_default();

    let raw = format!(
        "os:{};arch:{};rustc:{};kernel:{};mlx_model_dir:{};cargo_pkg_name:{}",
        os,
        arch,
        rustc_version.trim(),
        kernel_version.trim(),
        mlx_model_dir,
        cargo_pkg_name
    );
    blake3::hash(raw.as_bytes()).to_hex().to_string()
}

#[derive(Clone, Debug)]
pub struct PipelineContext {
    pub seed: u64,
    pub bias_version: BiasVersion,
    pub task_id: Option<String>,
}

pub trait PipelineStage {
    type Input;
    type Output;
    fn run(&self, input: Self::Input, ctx: &PipelineContext) -> Result<Self::Output>;
}

#[derive(Clone, Serialize)]
pub struct RawInput {
    pub payload: String,
}

#[derive(Clone, Serialize)]
pub struct IntermediateRepresentation {
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub steps: Vec<String>,
    pub seed: u64,
    pub spec: crate::exec_spec::ExecSpec,
}

impl Plan {
    /// P3: CodeFix steps form a STRICT sequential protocol (read → locate →
    /// patch → apply → tests → validate). The SemanticMapper deliberately
    /// tie-breaks equal-priority steps by seeded hash, which can scramble
    /// that order and let e.g. validate_patch run before the patch exists.
    /// When EVERY step of a plan is a CodeFix-flow kind, restore the
    /// canonical flow order (stable within repeated kinds). Non-CodeFix
    /// plans are untouched, so their stable IDs and replay behavior are
    /// unchanged.
    fn canonicalize_codefix_order(steps: Vec<String>) -> Vec<String> {
        use crate::workflow::contract::StepKind;
        use crate::workflow::planner::normalize_step;

        fn rank(kind: &StepKind) -> Option<u8> {
            match kind {
                StepKind::ReadRepository => Some(0),
                StepKind::LocateBug => Some(1),
                StepKind::PatchCode => Some(2),
                StepKind::ApplyPatch => Some(3),
                StepKind::RunTests => Some(4),
                StepKind::ValidatePatch => Some(5),
                _ => None,
            }
        }

        let kinds: Vec<Option<StepKind>> = steps.iter().map(|s| normalize_step(s)).collect();
        let all_codefix = !steps.is_empty()
            && kinds
                .iter()
                .all(|k| k.as_ref().map(|k| rank(k).is_some()).unwrap_or(false));
        if !all_codefix {
            return steps;
        }

        let mut indexed: Vec<(u8, usize, String)> = steps
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let r = normalize_step(&s)
                    .as_ref()
                    .and_then(rank)
                    .expect("checked above");
                (r, i, s)
            })
            .collect();
        indexed.sort_by_key(|(r, i, _)| (*r, *i));
        indexed.into_iter().map(|(_, _, s)| s).collect()
    }

    /// Deterministic BLAKE3-based ID from seed + steps
    pub fn new_with_stable_id(seed: u64, steps: Vec<String>) -> Self {
        let steps = Self::canonicalize_codefix_order(steps);

        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed.to_le_bytes());
        for step in &steps {
            hasher.update(step.as_bytes());
        }
        let id = hasher.finalize().to_hex()[..16].to_string();

        let workflow_steps: Vec<crate::workflow::contract::Step> = steps
            .iter()
            .map(|desc| {
                let kind = crate::workflow::planner::normalize_step(desc)
                    .unwrap_or(crate::workflow::contract::StepKind::ExecuteChanges);
                crate::workflow::contract::Step {
                    kind,
                    detail: Some(desc.clone()),
                }
            })
            .collect();
        let spec = crate::workflow::contract::steps_to_exec_spec(&workflow_steps);

        Plan {
            id,
            steps,
            seed,
            spec,
        }
    }
}

pub mod critic;
pub mod llm_critique;
pub mod llm_decompose;
pub mod normalizer;
pub mod parser;
pub mod semantic_mapper;
pub mod step_verifier;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_is_deterministic() {
        let steps = vec!["step one".to_string(), "step two".to_string()];
        let p1 = Plan::new_with_stable_id(42, steps.clone());
        let p2 = Plan::new_with_stable_id(42, steps);
        assert_eq!(p1.id, p2.id);
    }

    #[test]
    fn stable_id_differs_on_different_seed() {
        let steps = vec!["step one".to_string()];
        let p1 = Plan::new_with_stable_id(1, steps.clone());
        let p2 = Plan::new_with_stable_id(2, steps);
        assert_ne!(p1.id, p2.id);
    }

    #[test]
    fn stable_id_is_16_chars() {
        let p = Plan::new_with_stable_id(0, vec!["x".into()]);
        assert_eq!(p.id.len(), 16);
    }

    #[test]
    fn codefix_plan_keeps_canonical_order_for_any_seed() {
        // P3: regardless of seed (which the SemanticMapper uses to
        // tie-break), a plan whose steps are ALL CodeFix-flow kinds must end
        // up in the canonical sequential order read→locate→patch→apply→
        // tests→validate. Otherwise validate_patch could be scheduled before
        // any patch exists.
        let shuffled = vec![
            "validate patch".to_string(),
            "locate bug".to_string(),
            "patch code".to_string(),
            "read repository".to_string(),
            "apply patch".to_string(),
            "run tests".to_string(),
        ];
        let expected_kinds = vec![
            "ReadRepository",
            "LocateBug",
            "PatchCode",
            "ApplyPatch",
            "RunTests",
            "ValidatePatch",
        ];
        for seed in [1u64, 17, 42, 99, 12345] {
            let plan = Plan::new_with_stable_id(seed, shuffled.clone());
            let kinds: Vec<String> = plan
                .spec
                .steps
                .iter()
                .map(|s| {
                    s.primitive
                        .as_ref()
                        .and_then(|p| p.payload.get("step_kind"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string()
                })
                .collect();
            assert_eq!(
                kinds, expected_kinds,
                "canonical order broken at seed {seed}"
            );
        }
    }

    #[test]
    fn non_codefix_plan_order_is_untouched() {
        // A plan mixing non-CodeFix kinds must NOT be reordered (stable IDs
        // and replay behavior stay exactly as before P3).
        let steps = vec!["step one".to_string(), "step two".to_string()];
        let plan = Plan::new_with_stable_id(7, steps.clone());
        assert_eq!(plan.steps, steps);
    }

    #[test]
    fn question_plan_spec_uses_answer_question_step() {
        // P0 (H-2 fix): interrogative payloads must not be forced through
        // ExecuteChanges at plan construction time.
        let plan = Plan::new_with_stable_id(17, vec!["What is 17 × 19?".to_string()]);
        assert_eq!(plan.spec.steps.len(), 1);
        assert_eq!(plan.spec.steps[0].step_id, "00_answer_question");
        assert_eq!(
            plan.spec.steps[0].metadata["step_kind"],
            serde_json::json!("AnswerQuestion")
        );
        let prim = plan.spec.steps[0]
            .primitive
            .as_ref()
            .expect("question step must carry a primitive");
        assert_eq!(prim.payload["requires_llm"], serde_json::json!(true));
        assert_eq!(
            prim.payload["step_kind"],
            serde_json::json!("AnswerQuestion")
        );
    }

    #[test]
    fn russian_question_plan_spec_uses_answer_question_step() {
        let plan = Plan::new_with_stable_id(
            17,
            vec![
                "На складе было 7 насосов, 3 забрали. Сколько осталось? Ответь по-русски."
                    .to_string(),
            ],
        );
        assert_eq!(plan.spec.steps[0].step_id, "00_answer_question");
    }

    #[test]
    fn imperative_plan_still_defaults_to_execute_changes() {
        // Conservative default preserved for non-interrogative payloads.
        let plan = Plan::new_with_stable_id(17, vec!["Refactor the scheduler module".to_string()]);
        assert_eq!(plan.spec.steps[0].step_id, "00_execute_changes");
    }

    #[test]
    fn domain_task_mentioning_test_is_not_routed_to_hardening_stub_r1() {
        // R1 regression: the B2 acceptance payload mentions "test" several
        // times but is a domain logic puzzle. Before the fix it became a
        // single add_planner_test_coverage step (a computed stub that never
        // asks the model). It must now reach the model via ExecuteChanges.
        let b2 = "Five machines produce parts. Exactly one machine produces defective parts. \
                  You have one test that identifies whether a batch contains a defect. \
                  Design the minimum-test strategy if the machines can be grouped.";
        let plan = Plan::new_with_stable_id(42, vec![b2.to_string()]);
        assert_eq!(plan.spec.steps.len(), 1);
        let kind = plan.spec.steps[0]
            .primitive
            .as_ref()
            .and_then(|p| p.payload.get("step_kind"))
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        assert_ne!(
            kind, "AddPlannerTestCoverage",
            "domain payload must not be routed to the hardening stub"
        );
        assert_eq!(kind, "ExecuteChanges");
        let requires_llm = plan.spec.steps[0]
            .primitive
            .as_ref()
            .and_then(|p| p.payload.get("requires_llm"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        assert!(requires_llm, "the model must actually be asked");
    }
}
pub mod execution_engine;
pub mod persistence;
pub mod pipeline;
pub mod plan_diff;
pub mod replay;

pub mod report;

/// Строит план и публикует события в EventBus.
/// Эмитит pipeline.started, pipeline.stage.*, pipeline.completed или pipeline.failed.
///
/// Event publication is part of the kernel's durability contract: failures
/// are propagated to the caller instead of being swallowed (audit findings
/// C7/H5 — a silently dropped publish made the event log incomplete with no
/// error surface).
pub fn build_plan_and_publish(payload: &str, seed: u64, db_path: &str) -> Result<PipelineReport> {
    use crate::event_bus::EventBus;
    let bus = EventBus::new(db_path)?;
    match build_plan(payload, seed) {
        Ok(report) => {
            let stage_tuples: Vec<(String, String, u128)> = report
                .stage_events
                .iter()
                .map(|e| {
                    (
                        e.stage.to_string(),
                        e.description.clone(),
                        e.timestamp_offset_ms,
                    )
                })
                .collect();
            bus.publish_pipeline_report(
                &report.plan.id,
                &report.plan.id,
                report.plan.seed,
                report.planner_version,
                &report.plan.steps,
                &report.fingerprint,
                report.elapsed_ms,
                report.critic_report.passed,
                &report.critic_report.warnings,
                &stage_tuples,
            )
            .map_err(|e| anyhow::anyhow!("pipeline event publish failed: {e}"))?;
            // Сохраняем PipelineReport как артефакт для Replay.
            // Тип должен соответствовать CHECK-констрейнту схемы
            // ('pipeline_report'); значение 'pipeline_report_v1' отвергалось
            // схемой и молча терялось (audit finding H5).
            let artifact_payload = serde_json::json!({
                "fingerprint": report.fingerprint,
                "seed": report.plan.seed,
                "steps": report.plan.steps,
                "planner_version": report.planner_version,
                "elapsed_ms": report.elapsed_ms,
                "critic_passed": report.critic_report.passed,
                "warnings": report.critic_report.warnings,
            });
            bus.append_semantic_artifact(
                &report.plan.id,
                "pipeline",
                0,
                "pipeline_report",
                &artifact_payload,
            )
            .map_err(|e| anyhow::anyhow!("pipeline report artifact persist failed: {e}"))?;
            Ok(report)
        }
        Err(e) => {
            // Best-effort failure marker: if even this publish fails, the
            // original planning error is the more useful signal.
            let _ = bus.publish_pipeline_failed(
                "unknown",
                seed,
                env!("CARGO_PKG_VERSION"),
                &e.to_string(),
            );
            Err(e)
        }
    }
}

use crate::planner_pipeline::critic::PlannerCritic;
use crate::planner_pipeline::normalizer::Normalizer;
use crate::planner_pipeline::parser::Parser;
use crate::planner_pipeline::replay::ReplayTape;
use crate::planner_pipeline::report::{PipelineReport, ReplayEvent, StageName};
use crate::planner_pipeline::semantic_mapper::SemanticMapper;
use crate::semantic_bias::BiasConfiguration;
use std::time::Instant;

/// Library API. Pure function — no side effects.
/// Used by CLI, Scheduler, Worker, and future HTTP API.
pub fn build_plan(payload: &str, seed: u64) -> Result<PipelineReport> {
    let started = Instant::now();
    let ctx = PipelineContext {
        seed,
        bias_version: BiasVersion::V1,
        task_id: None,
    };
    let bias = BiasConfiguration::new("default", vec![]);
    let raw = RawInput {
        payload: payload.to_owned(),
    };
    let mut events: Vec<ReplayEvent> = Vec::new();

    let normalized = Normalizer.run(raw, &ctx)?;
    events.push(ReplayEvent {
        stage: StageName::Normalizer,
        timestamp_offset_ms: started.elapsed().as_millis(),
        description: "payload normalized".into(),
    });

    let ir = Parser.run(normalized, &ctx)?;
    events.push(ReplayEvent {
        stage: StageName::Parser,
        timestamp_offset_ms: started.elapsed().as_millis(),
        description: format!("parsed {} step(s)", ir.steps.len()),
    });

    let mapped = SemanticMapper { bias }.run(ir, &ctx)?;
    events.push(ReplayEvent {
        stage: StageName::SemanticMapper,
        timestamp_offset_ms: started.elapsed().as_millis(),
        description: format!("mapped {} step(s)", mapped.steps.len()),
    });

    let plan = Plan::new_with_stable_id(ctx.seed, mapped.steps);
    events.push(ReplayEvent {
        stage: StageName::StableId,
        timestamp_offset_ms: started.elapsed().as_millis(),
        description: format!("plan id={}", plan.id),
    });

    let critic_report = PlannerCritic.analyze(&plan);
    events.push(ReplayEvent {
        stage: StageName::Critic,
        timestamp_offset_ms: started.elapsed().as_millis(),
        description: if critic_report.passed {
            "critic passed".into()
        } else {
            format!("{} violation(s)", critic_report.invariant_violations.len())
        },
    });

    let mut replay_tape = ReplayTape::new();
    replay_tape.record(payload, seed, &plan.id);

    Ok(PipelineReport {
        fingerprint: get_environment_fingerprint(),
        plan,
        critic_report,
        replay_tape,
        stage_events: events,
        planner_version: env!("CARGO_PKG_VERSION"),
        elapsed_ms: started.elapsed().as_millis(),
    })
}

/// Build plan with silent LLM critique.
/// Deterministic pipeline runs first, then LLM decomposition (verified mode only),
/// then LLM critique (best-effort, never blocks).
pub async fn build_plan_with_silent_critique(payload: &str, seed: u64) -> Result<PipelineReport> {
    let mut report = build_plan(payload, seed)?;

    // LLM-assisted decomposition: if deterministic parser returned only 1 step,
    // try to decompose via LLM. Only useful in verified mode (LLM available).
    if report.plan.steps.len() == 1 {
        let original_steps = report.plan.steps.clone();
        match llm_decompose::llm_assisted_decompose(payload).await {
            Ok(Some(decomposed_steps)) if decomposed_steps.len() > 1 => {
                // Rebuild plan with LLM-decomposed steps
                let new_plan = Plan::new_with_stable_id(report.plan.seed, decomposed_steps);
                report.plan = new_plan;
                report
                    .stage_events
                    .push(crate::planner_pipeline::report::ReplayEvent {
                        stage: crate::planner_pipeline::report::StageName::LlmCritique,
                        timestamp_offset_ms: 0,
                        description: format!(
                            "llm_decompose: {} -> {} steps",
                            original_steps.len(),
                            report.plan.steps.len()
                        ),
                    });
            }
            _ => {
                // LLM decomposition not available or returned single step — keep original
            }
        }
    }

    // Run silent LLM critique (best-effort, never fails the pipeline)
    llm_critique::run_silent_critique(payload, &mut report).await;

    Ok(report)
}

/// Build plan with silent critique AND publish to EventBus.
pub async fn build_plan_with_critique_and_publish(
    payload: &str,
    seed: u64,
    db_path: &str,
) -> Result<PipelineReport> {
    use crate::event_bus::EventBus;
    let bus = EventBus::new(db_path)?;

    match build_plan_with_silent_critique(payload, seed).await {
        Ok(report) => {
            let stage_tuples: Vec<(String, String, u128)> = report
                .stage_events
                .iter()
                .map(|e| {
                    (
                        e.stage.to_string(),
                        e.description.clone(),
                        e.timestamp_offset_ms,
                    )
                })
                .collect();
            let _ = bus.publish_pipeline_report(
                &report.plan.id,
                &report.plan.id,
                report.plan.seed,
                report.planner_version,
                &report.plan.steps,
                &report.fingerprint,
                report.elapsed_ms,
                report.critic_report.passed,
                &report.critic_report.warnings,
                &stage_tuples,
            );
            let artifact_payload = serde_json::json!({
                "fingerprint": report.fingerprint,
                "seed": report.plan.seed,
                "steps": report.plan.steps,
                "planner_version": report.planner_version,
                "elapsed_ms": report.elapsed_ms,
                "critic_passed": report.critic_report.passed,
                "warnings": report.critic_report.warnings,
            });
            let _ = bus.append_semantic_artifact(
                &report.plan.id,
                "pipeline",
                0,
                "pipeline_report_v1",
                &artifact_payload,
            );
            Ok(report)
        }
        Err(e) => {
            let _ = bus.publish_pipeline_failed(
                "unknown",
                seed,
                env!("CARGO_PKG_VERSION"),
                &e.to_string(),
            );
            Err(e)
        }
    }
}
