use crate::planner_pipeline::critic::CriticReport;
use crate::planner_pipeline::replay::ReplayTape;
use crate::planner_pipeline::Plan;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum StageName {
    Normalizer,
    Parser,
    SemanticMapper,
    StableId,
    Critic,
}

impl std::fmt::Display for StageName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            StageName::Normalizer => "normalizer",
            StageName::Parser => "parser",
            StageName::SemanticMapper => "semantic_mapper",
            StageName::StableId => "stable_id",
            StageName::Critic => "critic",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayEvent {
    pub stage: StageName,
    pub timestamp_offset_ms: u128,
    pub description: String,
}

/// Full output of a single `build_plan()` call.
/// Invariant: all fields belong to the same run.
/// Side-effect-free: callers decide what to do with it.
#[derive(Debug, Serialize)]
pub struct PipelineReport {
    pub plan: Plan,
    pub critic_report: CriticReport,
    pub replay_tape: ReplayTape,
    pub stage_events: Vec<ReplayEvent>,
    pub fingerprint: String,
    pub planner_version: &'static str,
    pub elapsed_ms: u128,
}
