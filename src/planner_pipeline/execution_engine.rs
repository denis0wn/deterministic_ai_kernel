use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::planner_pipeline::pipeline::Pipeline;
use crate::planner_pipeline::replay::{ReplayTape, Replayer};
use crate::planner_pipeline::PipelineContext;

// ── Step-level result ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StepStatus {
    Ok,
    Skipped,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub index: usize,
    pub description: String,
    pub status: StepStatus,
    pub duration_ms: u64,
}

// ── Execution report ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub plan_id: String,
    pub seed: u64,
    pub steps: Vec<StepResult>,
    pub total_duration_ms: u64,
    pub success: bool,
}

impl ExecutionReport {
    pub fn failed_steps(&self) -> Vec<&StepResult> {
        self.steps
            .iter()
            .filter(|s| matches!(s.status, StepStatus::Failed(_)))
            .collect()
    }
    pub fn skipped_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == StepStatus::Skipped)
            .count()
    }
}

// ── Step executor trait (injectable / testable) ──────────────────────────────

pub trait StepExecutor: Send + Sync {
    fn execute(&self, index: usize, description: &str) -> Result<StepStatus>;
}

/// Default executor: validates the step description is non-empty, then marks Ok.
pub struct DefaultStepExecutor;

impl StepExecutor for DefaultStepExecutor {
    fn execute(&self, _index: usize, description: &str) -> Result<StepStatus> {
        if description.trim().is_empty() {
            bail!("step description is empty");
        }
        Ok(StepStatus::Ok)
    }
}

// ── Engine ───────────────────────────────────────────────────────────────────

pub struct ExecutionEngine {
    pipeline: Pipeline,
    executor: Box<dyn StepExecutor>,
}

impl ExecutionEngine {
    pub fn new(pipeline: Pipeline, executor: Box<dyn StepExecutor>) -> Self {
        Self { pipeline, executor }
    }

    pub fn with_default_executor(pipeline: Pipeline) -> Self {
        Self::new(pipeline, Box::new(DefaultStepExecutor))
    }

    /// Run pipeline → execute every step → return ExecutionReport.
    pub fn run(&self, payload: &str, ctx: &PipelineContext) -> Result<ExecutionReport> {
        let t0 = Instant::now();

        let out = self.pipeline.run(payload, ctx)?;
        let plan = &out.plan;

        let mut steps = Vec::with_capacity(plan.steps.len());
        let mut success = true;

        for (i, desc) in plan.steps.iter().enumerate() {
            let step_t = Instant::now();
            let status = match self.executor.execute(i, desc) {
                Ok(s) => s,
                Err(e) => {
                    success = false;
                    StepStatus::Failed(e.to_string())
                }
            };
            if matches!(status, StepStatus::Failed(_)) {
                success = false;
            }
            steps.push(StepResult {
                index: i,
                description: desc.clone(),
                status,
                duration_ms: step_t.elapsed().as_millis() as u64,
            });
        }

        Ok(ExecutionReport {
            plan_id: plan.id.clone(),
            seed: ctx.seed,
            steps,
            total_duration_ms: t0.elapsed().as_millis() as u64,
            success,
        })
    }

    /// Run + record to tape + verify replay consistency.
    pub fn run_with_replay(
        &self,
        payload: &str,
        ctx: &PipelineContext,
        tape: &mut ReplayTape,
    ) -> Result<ExecutionReport> {
        let report = self.run(payload, ctx)?;
        tape.record(payload, ctx.seed, &report.plan_id);

        // Immediately verify last entry is still stable
        let verifier = Replayer::new(Pipeline::new(self.pipeline.bias.clone()));
        let single = {
            let mut t = ReplayTape::new();
            t.record(payload, ctx.seed, &report.plan_id);
            t
        };
        verifier.verify(&single)?;

        Ok(report)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_pipeline::pipeline::Pipeline;
    use crate::planner_pipeline::PipelineContext;
    use crate::semantic_bias::{BiasConfiguration, BiasVersion, SemanticBiasRule};

    fn bias() -> BiasConfiguration {
        BiasConfiguration::new(
            "test",
            vec![SemanticBiasRule::new("r1", 1, "critical", "first")],
        )
    }
    fn ctx() -> PipelineContext {
        PipelineContext {
            seed: 42,
            bias_version: BiasVersion::V1,
        }
    }
    fn engine() -> ExecutionEngine {
        ExecutionEngine::with_default_executor(Pipeline::new(bias()))
    }

    #[test]
    fn runs_all_steps_successfully() {
        let r = engine()
            .run("step one\nstep two\ncritical step", &ctx())
            .unwrap();
        assert!(r.success);
        assert_eq!(r.steps.len(), 3);
        assert!(r.failed_steps().is_empty());
    }

    #[test]
    fn report_plan_id_matches_pipeline() {
        let p = Pipeline::new(bias());
        let out = p.run("step one\nstep two", &ctx()).unwrap();
        let eng = ExecutionEngine::with_default_executor(Pipeline::new(bias()));
        let r = eng.run("step one\nstep two", &ctx()).unwrap();
        assert_eq!(r.plan_id, out.plan.id);
    }

    #[test]
    fn report_seed_is_preserved() {
        let r = engine().run("step one\nstep two", &ctx()).unwrap();
        assert_eq!(r.seed, 42);
    }

    #[test]
    fn failing_executor_marks_report_failed() {
        struct AlwaysFail;
        impl StepExecutor for AlwaysFail {
            fn execute(&self, _i: usize, _d: &str) -> Result<StepStatus> {
                Ok(StepStatus::Failed("boom".into()))
            }
        }
        let eng = ExecutionEngine::new(Pipeline::new(bias()), Box::new(AlwaysFail));
        let r = eng.run("step one\nstep two", &ctx()).unwrap();
        assert!(!r.success);
        assert_eq!(r.failed_steps().len(), 2);
    }

    #[test]
    fn skipping_executor_counts_correctly() {
        struct AllSkip;
        impl StepExecutor for AllSkip {
            fn execute(&self, _i: usize, _d: &str) -> Result<StepStatus> {
                Ok(StepStatus::Skipped)
            }
        }
        let eng = ExecutionEngine::new(Pipeline::new(bias()), Box::new(AllSkip));
        let r = eng.run("step one\nstep two\nstep three", &ctx()).unwrap();
        assert_eq!(r.skipped_count(), 3);
        // skipped ≠ failed → success still true
        assert!(r.success);
    }

    #[test]
    fn run_is_deterministic_same_seed() {
        let r1 = engine().run("alpha\nbeta\ngamma", &ctx()).unwrap();
        let r2 = engine().run("alpha\nbeta\ngamma", &ctx()).unwrap();
        assert_eq!(r1.plan_id, r2.plan_id);
        assert_eq!(r1.steps.len(), r2.steps.len());
    }

    #[test]
    fn run_with_replay_verifies_consistency() {
        let mut tape = ReplayTape::new();
        let eng = engine();
        eng.run_with_replay("step one\nstep two", &ctx(), &mut tape)
            .unwrap();
        assert_eq!(tape.len(), 1);
    }

    #[test]
    fn run_with_replay_detects_tamper() {
        // Build a tape with a wrong plan_id manually, then verify fails
        let mut tape = ReplayTape::new();
        tape.record("step one\nstep two", 42, "0000000000000000");
        let verifier = crate::planner_pipeline::replay::Replayer::new(Pipeline::new(bias()));
        assert!(verifier.verify(&tape).is_err());
    }

    #[test]
    fn step_results_have_correct_indices() {
        let r = engine().run("a\nb\nc", &ctx()).unwrap();
        for (i, s) in r.steps.iter().enumerate() {
            assert_eq!(s.index, i);
        }
    }
}
