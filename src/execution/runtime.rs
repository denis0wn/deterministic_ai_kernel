use crate::providers;
use anyhow::Result;

#[derive(Default)]
pub struct Runtime;

impl Runtime {
    pub fn new() -> Self {
        Self
    }

    pub fn execute_step(&self, task_id: &str, step_id: &str, payload: &str) -> Result<()> {
        let storage = providers::get_storage();
        let spec = storage.load_exec_spec(task_id)?;

        if let Some(step_spec) = spec.steps.iter().find(|s| s.step_id == step_id) {
            if let Some(ref prim) = step_spec.primitive {
                let result = crate::execution::primitive_executor::PrimitiveExecutor::execute(
                    task_id, prim, payload,
                )?;

                // Generic recording of returned artifacts to preserve domain-agnostic boundary
                let generation = storage.latest_generation_for_task(task_id)?;
                for artifact in result.artifacts {
                    storage.append_semantic_artifact(
                        task_id,
                        step_id,
                        generation,
                        &artifact.artifact_type,
                        &artifact.payload,
                    )?;
                }
            }
        }

        Ok(())
    }
}
