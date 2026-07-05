#![allow(dead_code, unused)]
use anyhow::Result;
use serde_json::json;

use crate::embeddings::embed_text;
use crate::event_bus::EventBus;
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
