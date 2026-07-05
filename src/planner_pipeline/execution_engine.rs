use std::time::Instant;
use anyhow::Result;

use crate::planner_pipeline::{Plan, PipelineContext, RawInput};
use crate::planner_pipeline::pipeline::{Pipeline, PipelineOutput};
use crate::planner_pipeline::replay::ReplayTape;

// ── Step executor trait ───────────────────────────────────────────────────────

/// Receives a step description, returns Ok(output) or Err(reason).
pub trait StepExecutor: Send + Sync {
    fn execute(&self, step_description: &str) -> Result<String>;
}

/// Default executor — records the step as a no-op (used in tests / dry-run).
pub struct NoOpExecutor;

impl StepExecutor for NoOpExecutor {
    fn execute(&self, step: &str) -> Result<String> {
        Ok(format!("completed: {step}"))
    }
}

/// Executor that always fails (useful for testing failure paths).
pub struct FailingExecutor {
    pub reason: String,
}

impl StepExecutor for FailingExecutor {
    fn execute(&self, _step: &str) -> Result<String> {
        anyhow::bail!("{}", self.reason)
    }
}

// ── Execution events ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StepEvent {
    pub step_index: usize,
    pub description: String,
    pub success: bool,
    pub output: String,
    pub duration_ms: u64,
}

// ── ExecutionReport ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExecutionReport {
    pub plan_id: String,
    pub success: bool,
    pub completed_steps: Vec<String>,
    pub failed_steps: Vec<String>,
    pub events: Vec<StepEvent>,
    pub duration_ms: u64,
}

impl ExecutionReport {
    pub fn completed_count(&self) -> usize { self.completed_steps.len() }
    pub fn failed_count(&self) -> usize { self.failed_steps.len() }
}

// ── ExecutionEngine ───────────────────────────────────────────────────────────

pub struct ExecutionEngine<E: StepExecutor> {
    pipeline: Pipeline,
    executor: E,
}

impl<E: StepExecutor> ExecutionEngine<E> {
    pub fn new(pipeline: Pipeline, executor: E) -> Self {
        Self { pipeline, executor }
    }

    /// Build a plan from `payload`, execute each step, return an ExecutionReport.
    /// On executor error the engine records the failure and stops (fail-fast).
    pub fn run(
        &self,
        payload: impl Into<String>,
        ctx: &PipelineContext,
        tape: Option<&mut ReplayTape>,
    ) -> Result<ExecutionReport> {
        let payload_str: String = payload.into();

        // 1. Build plan via pipeline
        let PipelineOutput { plan, report: critic_report } =
            self.pipeline.run(payload_str.clone(), ctx)?;

        if !critic_report.passed {
            anyhow::bail!(
                "Critic rejected plan {}: {:?}",
                plan.id,
                critic_report.invariant_violations
            );
        }

        // 2. Execute steps
        let total_start = Instant::now();
        let mut events: Vec<StepEvent> = Vec::with_capacity(plan.steps.len());
        let mut completed: Vec<String> = vec![];
        let mut failed: Vec<String> = vec![];
        let mut success = true;

        for (idx, step) in plan.steps.iter().enumerate() {
            let step_start = Instant::now();
            match self.executor.execute(step) {
                Ok(output) => {
                    let dur = step_start.elapsed().as_millis() as u64;
                    events.push(StepEvent {
                        step_index: idx,
                        description: step.clone(),
                        success: true,
                        output,
                        duration_ms: dur,
                    });
                    completed.push(step.clone());
                }
                Err(e) => {
                    let dur = step_start.elapsed().as_millis() as u64;
                    events.push(StepEvent {
                        step_index: idx,
                        description: step.clone(),
                        success: false,
                        output: e.to_string(),
                        duration_ms: dur,
                    });
                    failed.push(step.clone());
                    success = false;
                    break; // fail-fast
                }
            }
        }

        let duration_ms = total_start.elapsed().as_millis() as u64;

        // 3. Record to tape if provided (only on full success)
        if success {
            if let Some(t) = tape {
                t.record(&payload_str, ctx.seed, &plan.id);
            }
        }

        Ok(ExecutionReport {
            plan_id: plan.id,
            success,
            completed_steps: completed,
            failed_steps: failed,
            events,
            duration_ms,
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_pipeline::{PipelineContext};
    use crate::planner_pipeline::pipeline::Pipeline;
    use crate::planner_pipeline::replay::ReplayTape;
    use crate::semantic_bias::{BiasConfiguration, BiasVersion, SemanticBiasRule};

    fn bias() -> BiasConfiguration {
        BiasConfiguration::new("test", vec![SemanticBiasRule::new("r1", 1, "critical", "first")])
    }
    fn ctx() -> PipelineContext { PipelineContext { seed: 42, bias_version: BiasVersion::V1 } }

    fn engine() -> ExecutionEngine<NoOpExecutor> {
        ExecutionEngine::new(Pipeline::new(bias()), NoOpExecutor)
    }

    #[test]
    fn engine_executes_all_steps() {
        let report = engine().run("step one\nstep two\ncritical step", &ctx(), None).unwrap();
        assert!(report.success);
        assert_eq!(report.completed_count(), 3);
        assert_eq!(report.failed_count(), 0);
    }

    #[test]
    fn engine_report_has_plan_id() {
        let report = engine().run("step one\nstep two", &ctx(), None).unwrap();
        assert_eq!(report.plan_id.len(), 16);
    }

    #[test]
    fn engine_events_match_steps() {
        let report = engine().run("step one\nstep two", &ctx(), None).unwrap();
        assert_eq!(report.events.len(), 2);
        assert!(report.events.iter().all(|e| e.success));
    }

    #[test]
    fn engine_fail_fast_on_executor_error() {
        let bad = ExecutionEngine::new(
            Pipeline::new(bias()),
            FailingExecutor { reason: "boom".into() },
        );
        let report = bad.run("step one\nstep two\nstep three", &ctx(), None).unwrap();
        assert!(!report.success);
        assert_eq!(report.completed_count(), 0);
        assert_eq!(report.failed_count(), 1);
        assert_eq!(report.events.len(), 1);
    }

    #[test]
    fn engine_records_to_tape_on_success() {
        let mut tape = ReplayTape::new();
        engine().run("step one\nstep two", &ctx(), Some(&mut tape)).unwrap();
        assert_eq!(tape.len(), 1);
        assert_eq!(tape.entries()[0].seed, 42);
    }

    #[test]
    fn engine_does_not_record_to_tape_on_failure() {
        let mut tape = ReplayTape::new();
        let bad = ExecutionEngine::new(
            Pipeline::new(bias()),
            FailingExecutor { reason: "fail".into() },
        );
        bad.run("step one\nstep two", &ctx(), Some(&mut tape)).unwrap();
        assert!(tape.is_empty());
    }

    #[test]
    fn engine_rejects_invalid_plan() {
        let result = engine().run("  \n  \n", &ctx(), None);
        assert!(result.is_err());
    }

    #[test]
    fn engine_duration_is_set() {
        let report = engine().run("step one\nstep two", &ctx(), None).unwrap();
        // duration_ms is a u64 — just verify it compiles and doesn't panic
        let _ = report.duration_ms;
    }
}
