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
        match step.kind {
            StepKind::AnalyzeTask => {
                let detail = step.detail.as_deref().unwrap_or("analyze task").trim();
                let vector = embed_text(detail).await?;

                self.bus.append_event(
                    task_id,
                    None,
                    "ANALYZE_TASK_EMBEDDED",
                    &json!({
                        "step": step.as_text(),
                        "input_representation": detail,
                        "embedding_dim": vector.len(),
                        "analysis_kind": "semantic_seed"
                    }),
                )?;
            }
            _ => {}
        }

        Ok(())
    }
}
