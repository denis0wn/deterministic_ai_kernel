use crate::planner_pipeline::{
    critic::{CriticReport, PlannerCritic},
    normalizer::Normalizer,
    parser::Parser,
    semantic_mapper::SemanticMapper,
    IntermediateRepresentation, PipelineContext, PipelineStage, Plan, RawInput,
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

fn query_mlx_server(payload: &str) -> Option<Vec<String>> {
    // 1. Check if backend is mocked
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        return None;
    }

    // 2. Resolve model config
    let config =
        crate::model_registry::resolve_model(crate::model_registry::ModelPurpose::TaskPlanning)
            .ok()?;

    // 3. Build blocking client with a short timeout
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    // 4. Test reachability (GET base_url/models)
    let models_url = format!("{}/models", config.base_url.trim_end_matches('/'));
    client.get(&models_url).send().ok()?;

    // 5. Send completions request
    let completions_url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let payload_json = serde_json::json!({
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": "You are a concise task planning assistant. Follow output constraints exactly. Return one step per line without numbering or bullet points."
            },
            {
                "role": "user",
                "content": format!("Generate short task execution steps for: {}", payload)
            }
        ],
        "temperature": 0.0
    });

    let res = client
        .post(completions_url)
        .bearer_auth(&config.api_key)
        .json(&payload_json)
        .send()
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    // 6. Parse response
    #[derive(serde::Deserialize)]
    struct Choice {
        message: Message,
    }
    #[derive(serde::Deserialize)]
    struct Message {
        content: String,
    }
    #[derive(serde::Deserialize)]
    struct Response {
        choices: Vec<Choice>,
    }

    let body = res.json::<Response>().ok()?;
    let text = body.choices.first()?.message.content.clone();

    let mut steps = Vec::new();
    for line in text.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches(|c: char| {
                c.is_ascii_digit() || c == '.' || c == '-' || c == ')' || c.is_whitespace()
            })
            .trim()
            .trim_end_matches('.')
            .to_string();
        if !trimmed.is_empty() {
            steps.push(trimmed);
        }
    }

    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
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

        // Stage 2: Parse (or query MLX Server)
        let mut steps = None;
        if let Some(mlx_steps) = query_mlx_server(&normalized.payload) {
            steps = Some(mlx_steps);
        }

        let ir = match steps {
            Some(s) => IntermediateRepresentation { steps: s },
            None => Parser.run(normalized, ctx)?,
        };

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
            task_id: None,
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
            .expect("test failure");
        assert!(out.report.passed);
        assert_eq!(out.plan.steps.len(), 3);
        assert_eq!(out.plan.seed, 42);
    }

    #[test]
    fn pipeline_id_is_stable_across_runs() {
        let pipeline = Pipeline::new(bias());
        let out1 = pipeline
            .run("step one\nstep two", &ctx())
            .expect("test failure");
        let out2 = pipeline
            .run("step one\nstep two", &ctx())
            .expect("test failure");
        assert_eq!(out1.plan.id, out2.plan.id);
    }

    #[test]
    fn pipeline_normalizes_before_parsing() {
        let pipeline = Pipeline::new(bias());
        let out = pipeline
            .run("  STEP ONE  \n\n  STEP TWO  ", &ctx())
            .expect("test failure");
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
            task_id: None,
        };
        let ctx2 = PipelineContext {
            seed: 2,
            bias_version: BiasVersion::V1,
            task_id: None,
        };
        let out1 = pipeline
            .run("step one\nstep two", &ctx1)
            .expect("test failure");
        let out2 = pipeline
            .run("step one\nstep two", &ctx2)
            .expect("test failure");
        assert_ne!(out1.plan.id, out2.plan.id);
    }

    #[test]
    fn publish_output_writes_to_event_bus() {
        let unique_db = |label: &str| -> String {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test failure")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("dak_test_{}_{}.db", label, nanos));
            let path_str = path.display().to_string();
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(format!("{}-wal", path_str));
            let _ = std::fs::remove_file(format!("{}-shm", path_str));
            path_str
        };

        let db_path = unique_db("publish_output");
        let bus = EventBus::new(&db_path).expect("test failure");
        let pipeline = Pipeline::new(bias());
        let ctx = ctx();
        let output = pipeline
            .run("step one\nstep two\ncritical step", &ctx)
            .expect("test failure");

        let gen = pipeline
            .publish_output(&output, "task-42", &bus)
            .expect("test failure");
        assert!(gen > 0);

        // EventBus содержит запись о pipeline_report
        let latest = bus
            .latest_generation_for_task("task-42")
            .expect("test failure");
        assert_eq!(latest, gen);

        // Semantic artifacts созданы для каждого шага
        let artifacts = bus
            .list_semantic_artifacts("task-42", None)
            .expect("test failure");
        assert_eq!(artifacts.len(), output.plan.steps.len());
        assert!(artifacts.iter().all(|a| a.artifact_type == "pipeline_step"));

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(format!("{}-wal", db_path));
        let _ = std::fs::remove_file(format!("{}-shm", db_path));
    }
}
