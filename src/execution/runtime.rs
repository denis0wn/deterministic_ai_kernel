#![allow(dead_code, unused)]
use anyhow::Result;
use serde_json::json;

use crate::embeddings::embed_text;
use crate::event_bus::EventBus;
use crate::kernel_types::{AIInput, AIRequest, AIResponse, AITrace, Modality};
use crate::model_registry::{resolve_model, ModelPurpose};
use crate::workflow::contract::{Step, StepKind};
use crate::workflow::pipeline::PipelineOutput;

pub struct Runtime {
    bus: EventBus,
}

impl Runtime {
    pub fn new(bus: EventBus) -> Self {
        Self { bus }
    }

    pub async fn execute_step(&self, task_id: &str, step: &Step) -> Result<()> {
        if step.kind == StepKind::AnalyzeTask {
            let detail = step.detail.as_deref().unwrap_or("analyze task").trim();

            let request = AIRequest {
                request_id: format!("{}::{}::request", task_id, step.as_text()),
                modality: Modality::Text,
                input: AIInput::Text {
                    prompt: detail.to_string(),
                },
                model: Some(resolve_model(ModelPurpose::TaskPlanning)?.model),
                deterministic: true,
            };

            let request_trace = AITrace {
                model_id: resolve_model(ModelPurpose::TaskPlanning)?.model,
                model_hash: "registry-selected".to_string(),
                prompt_hash: format!("prompt:{}", detail),
                sampling_config: "deterministic".to_string(),
                timestamp: 0,
                output_hash: "pending".to_string(),
            };

            self.bus
                .append_ai_request(task_id, Some(&step.as_text()), &request, &request_trace)?;

            let vector = embed_text(detail).await?;
            let source_generation = self.bus.latest_generation_for_task(task_id)?;

            self.bus.append_semantic_artifact(
                task_id,
                &step.as_text(),
                source_generation,
                "analysis_seed",
                &json!({
                    "input_representation": detail,
                    "embedding_dim": vector.len(),
                    "analysis_kind": "semantic_seed"
                }),
            )?;

            let response = AIResponse {
                request_id: request.request_id.clone(),
                model_id: resolve_model(ModelPurpose::TaskPlanning)?.model,
                output_text: format!("embedding_dim={}", vector.len()),
                finish_reason: Some("completed".to_string()),
            };

            let response_trace = AITrace {
                model_id: resolve_model(ModelPurpose::TaskPlanning)?.model,
                model_hash: "registry-selected".to_string(),
                prompt_hash: format!("prompt:{}", detail),
                sampling_config: "deterministic".to_string(),
                timestamp: 0,
                output_hash: format!("embedding:{}", vector.len()),
            };

            self.bus.append_ai_response(
                task_id,
                Some(&step.as_text()),
                &response,
                &response_trace,
            )?;
        }

        Ok(())
    }

    /// Execute all steps from a PipelineOutput, publishing events for each.
    pub async fn execute_plan(&self, output: &PipelineOutput) -> anyhow::Result<()> {
        for ps in &output.steps {
            self.execute_step(&output.task_id, &ps.step).await?;
        }
        Ok(())
    }
}
