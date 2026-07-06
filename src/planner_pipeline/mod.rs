use serde::Serialize;
use anyhow::Result;
use crate::semantic_bias::BiasVersion;

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

pub mod normalizer;
pub mod parser;
pub mod semantic_mapper;
pub mod critic;


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
pub mod pipeline;
pub mod replay;
pub mod plan_diff;
pub mod execution_engine;
pub mod persistence;

pub mod report;

use std::time::Instant;
use crate::planner_pipeline::critic::PlannerCritic;
use crate::planner_pipeline::normalizer::Normalizer;
use crate::planner_pipeline::parser::Parser;
use crate::planner_pipeline::report::{PipelineReport, ReplayEvent, StageName};
use crate::planner_pipeline::replay::ReplayTape;
use crate::planner_pipeline::semantic_mapper::SemanticMapper;
use crate::semantic_bias::BiasConfiguration;

/// Library API. Pure function — no side effects.
/// Used by CLI, Scheduler, Worker, and future HTTP API.
pub fn build_plan(payload: &str, seed: u64) -> Result<PipelineReport> {
    let started = Instant::now();
    let ctx = PipelineContext { seed, bias_version: BiasVersion::V1 };
    let bias = BiasConfiguration::new("default", vec![]);
    let raw = RawInput { payload: payload.to_owned() };
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

