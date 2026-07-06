use crate::planner_pipeline::{
    critic::{CriticReport, PlannerCritic},
    normalizer::Normalizer,
    parser::Parser,
    semantic_mapper::SemanticMapper,
    PipelineContext, PipelineStage, Plan, RawInput,
};
use crate::semantic_bias::BiasConfiguration;
use anyhow::Result;

pub struct Pipeline {
    pub bias: BiasConfiguration,
}

pub struct PipelineOutput {
    pub plan: Plan,
    pub report: CriticReport,
}

impl Pipeline {
    pub fn new(bias: BiasConfiguration) -> Self {
        Self { bias }
    }

    pub fn run(&self, payload: impl Into<String>, ctx: &PipelineContext) -> Result<PipelineOutput> {
        let raw = RawInput {
            payload: payload.into(),
        };

        // Stage 1: Normalize
        let normalized = Normalizer.run(raw, ctx)?;

        // Stage 2: Parse
        let ir = Parser.run(normalized, ctx)?;

        // Stage 3: Semantic mapping (bias + seed ordering)
        let mapped = SemanticMapper {
            bias: self.bias.clone(),
        }
        .run(ir, ctx)?;

        // Stage 4: Stable BLAKE3 plan ID
        let plan = Plan::new_with_stable_id(ctx.seed, mapped.steps);

        // Stage 5: Critic (analyze only, no mutation)
        let report = PlannerCritic.analyze(&plan);

        Ok(PipelineOutput { plan, report })
    }
}

use crate::event_bus::EventBus;

impl Pipeline {
    pub fn publish_output(
        &self,
        output: &PipelineOutput,
        task_id: &str,
        bus: &EventBus,
    ) -> anyhow::Result<i64> {
        let payload = serde_json::json!({
            "task_id": task_id,
            "plan_id": &output.plan.id,
            "seed": output.plan.seed,
            "steps": &output.plan.steps,
            "critic_passed": output.report.passed,
            "critic_violations": &output.report.invariant_violations,
            "critic_warnings": &output.report.warnings,
        });
        let generation = bus.append_event(task_id, None, "pipeline_report", &payload)?;

        for (i, step) in output.plan.steps.iter().enumerate() {
            let step_id = format!("step_{i}");
            let step_payload = serde_json::json!({
                "index": i,
                "text": step,
                "plan_id": &output.plan.id,
            });
            bus.append_semantic_artifact(
                task_id,
                &step_id,
                generation,
                "pipeline_step",
                &step_payload,
            )?;
        }

        Ok(generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventBus;
    use crate::semantic_bias::{BiasConfiguration, BiasVersion, SemanticBiasRule};

    fn ctx() -> PipelineContext {
        PipelineContext {
            seed: 42,
            bias_version: BiasVersion::V1,
        }
    }

    fn bias() -> BiasConfiguration {
        BiasConfiguration::new(
            "test-bias",
            vec![SemanticBiasRule::new("r1", 1, "critical", "first")],
        )
    }

    #[test]
    fn pipeline_produces_valid_plan() {
        let pipeline = Pipeline::new(bias());
        let out = pipeline
            .run("step one\nstep two\ncritical step", &ctx())
            .unwrap();
        assert!(out.report.passed);
        assert_eq!(out.plan.steps.len(), 3);
        assert_eq!(out.plan.seed, 42);
    }

    #[test]
    fn pipeline_id_is_stable_across_runs() {
        let pipeline = Pipeline::new(bias());
        let out1 = pipeline.run("step one\nstep two", &ctx()).unwrap();
        let out2 = pipeline.run("step one\nstep two", &ctx()).unwrap();
        assert_eq!(out1.plan.id, out2.plan.id);
    }

    #[test]
    fn pipeline_normalizes_before_parsing() {
        let pipeline = Pipeline::new(bias());
        let out = pipeline
            .run("  STEP ONE  \n\n  STEP TWO  ", &ctx())
            .unwrap();
        assert!(out
            .plan
            .steps
            .iter()
            .all(|s| s == s.to_lowercase().as_str()));
    }

    #[test]
    fn pipeline_critic_catches_empty_payload() {
        let pipeline = Pipeline::new(bias());
        let result = pipeline.run("   \n  \n", &ctx());
        assert!(result.is_err()); // Parser bails on empty
    }

    #[test]
    fn pipeline_different_seeds_produce_different_ids() {
        let pipeline = Pipeline::new(bias());
        let ctx1 = PipelineContext {
            seed: 1,
            bias_version: BiasVersion::V1,
        };
        let ctx2 = PipelineContext {
            seed: 2,
            bias_version: BiasVersion::V1,
        };
        let out1 = pipeline.run("step one\nstep two", &ctx1).unwrap();
        let out2 = pipeline.run("step one\nstep two", &ctx2).unwrap();
        assert_ne!(out1.plan.id, out2.plan.id);
    }

    #[test]
    fn publish_output_writes_to_event_bus() {
        let bus = EventBus::new(":memory:").unwrap();
        let pipeline = Pipeline::new(bias());
        let ctx = ctx();
        let output = pipeline
            .run("step one\nstep two\ncritical step", &ctx)
            .unwrap();

        let gen = pipeline.publish_output(&output, "task-42", &bus).unwrap();
        assert!(gen > 0);

        // EventBus содержит запись о pipeline_report
        let latest = bus.latest_generation_for_task("task-42").unwrap();
        assert_eq!(latest, gen);

        // Semantic artifacts созданы для каждого шага
        let artifacts = bus.list_semantic_artifacts("task-42", None).unwrap();
        assert_eq!(artifacts.len(), output.plan.steps.len());
        assert!(artifacts.iter().all(|a| a.artifact_type == "pipeline_step"));
    }
}
