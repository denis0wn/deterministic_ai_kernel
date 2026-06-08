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

                let token_estimate = detail.split_whitespace().count();
                let summary = if detail.len() > 160 {
                    format!("{}...", &detail[..160])
                } else {
                    detail.to_string()
                };

                self.bus.append_event(
                    task_id,
                    None,
                    "ANALYZE_TASK_EMBEDDED",
                    &json!({
                        "step": step.as_text(),
                        "detail": detail,
                        "summary": summary,
                        "embedding_dim": vector.len(),
                        "token_estimate": token_estimate,
                        "analysis_kind": "semantic_seed"
                    }),
                )?;
            }
            _ => {}
        }

        Ok(())
    }
}
