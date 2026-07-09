use anyhow::Result;
use serde_json::json;

use crate::embeddings::embed_text;
use crate::event_bus::EventBus;
use crate::workflow::contract::{Step, StepKind};

pub struct Runtime {
    bus: EventBus,
}

impl Runtime {
    pub fn new(bus: EventBus) -> Self {
        Self { bus }
    }

    pub async fn execute_step(&self, task_id: &str, step: &Step) -> Result<()> {
        let detail = step.detail.as_deref().unwrap_or("").trim();
        let source_generation = self.bus.latest_generation_for_task(task_id)?;

        match step.kind {
            StepKind::AnalyzeTask => {
                let text = if detail.is_empty() {
                    "analyze task"
                } else {
                    detail
                };
                let vector = embed_text(text).await?;

                self.bus.append_semantic_artifact(
                    task_id,
                    &step.as_text(),
                    source_generation,
                    "analysis_seed",
                    &json!({
                        "input_representation": text,
                        "embedding_dim": vector.len(),
                        "analysis_kind": "semantic_seed"
                    }),
                )?;
            }

            StepKind::PlanExecution => {
                self.bus.append_semantic_artifact(
                    task_id,
                    &step.as_text(),
                    source_generation,
                    "execution_plan_v1",
                    &json!({
                        "task_id": task_id,
                        "step": step.as_text(),
                        "detail": detail,
                        "status": "planned"
                    }),
                )?;
            }

            StepKind::ExecuteChanges => {
                let final_answer = detail.to_string();
                std::fs::create_dir_all("artifacts")?;
                std::fs::write(
                    format!("artifacts/final_answer.{}.txt", task_id),
                    format!("{}\n", final_answer),
                )?;

                self.bus.append_semantic_artifact(
                    task_id,
                    &step.as_text(),
                    source_generation,
                    "final_answer_v1",
                    &json!({
                        "task_id": task_id,
                        "answer": final_answer
                    }),
                )?;
            }

            _ => {
                self.bus.append_semantic_artifact(
                    task_id,
                    &step.as_text(),
                    source_generation,
                    "step_execution_v1",
                    &json!({
                        "task_id": task_id,
                        "step": step.as_text(),
                        "detail": detail,
                        "status": "executed"
                    }),
                )?;
            }
        }

        Ok(())
    }
}
