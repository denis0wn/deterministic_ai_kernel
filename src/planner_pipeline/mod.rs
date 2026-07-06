use crate::semantic_bias::BiasVersion;
use anyhow::Result;
use serde::Serialize;

pub struct PipelineContext {
    pub seed: u64,
    pub bias_version: BiasVersion,
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

#[derive(Debug, Serialize)]
pub struct Plan {
    pub id: String,
    pub steps: Vec<String>,
    pub seed: u64,
}

impl Plan {
    /// Deterministic BLAKE3-based ID from seed + steps
    pub fn new_with_stable_id(seed: u64, steps: Vec<String>) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed.to_le_bytes());
        for step in &steps {
            hasher.update(step.as_bytes());
        }
        let id = hasher.finalize().to_hex()[..16].to_string();
        Plan { id, steps, seed }
    }
}

pub mod critic;
pub mod normalizer;
pub mod parser;
pub mod semantic_mapper;

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
}
pub mod execution_engine;
pub mod persistence;
pub mod pipeline;
pub mod plan_diff;
pub mod replay;

pub mod report;

/// Строит план и публикует события в EventBus.
/// Эмитит pipeline.started, pipeline.stage.*, pipeline.completed или pipeline.failed.
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
            // Сохраняем PipelineReport как артефакт для Replay
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
        fingerprint: plan.id.clone(),
        plan,
        critic_report,
        replay_tape,
        stage_events: events,
        planner_version: env!("CARGO_PKG_VERSION"),
        elapsed_ms: started.elapsed().as_millis(),
    })
}
