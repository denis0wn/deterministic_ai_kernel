use anyhow::{anyhow, Result};

use crate::providers;
use crate::scheduler::{next_ready_step, schedule};
use crate::worker;

fn payload_for_task(task_id: &str) -> Result<String> {
    let path = format!("artifacts/pipeline_input.{}.txt", task_id);
    providers::get_filesystem().read_to_string(&path)
}

fn default_worker_for_step(step_id: &str) -> &'static str {
    let slug = step_id
        .split_once('_')
        .map(|(_, rest)| rest)
        .unwrap_or(step_id);

    match slug {
        "analyze_task" | "plan_execution" | "read_repository" | "locate_bug" => "worker-planner",
        "execute_changes" | "patch_code" | "run_tests" => "worker-executor",
        "validate_patch" | "validate_planner_output" => "worker-verifier",
        _ => "worker-planner",
    }
}

pub fn execute_effects(db: &str, task_id: &str) -> Result<()> {
    providers::get_filesystem().create_dir_all("artifacts")?;
    let payload = payload_for_task(task_id)?;

    let storage = providers::get_storage();
    let spec = storage.load_exec_spec(task_id)?;

    loop {
        schedule(db, task_id)?;

        let step_id = if let Some(step_id) = next_ready_step(db, task_id)? {
            step_id
        } else {
            let dispatched = storage.find_dispatched_step(task_id)?;
            match dispatched {
                Some(dispatched_step_id) => dispatched_step_id,
                None => break,
            }
        };

        let worker_id = default_worker_for_step(&step_id).to_string();

        worker::claim_worker(db, task_id, &worker_id)?;
        worker::start_step(db, task_id, &worker_id, &step_id)?;

        let mut executed_via_primitive = false;
        if let Some(step_spec) = spec.steps.iter().find(|s| s.step_id == step_id) {
            if let Some(ref prim) = step_spec.primitive {
                executed_via_primitive = true;

                match crate::execution::primitive_executor::PrimitiveExecutor::execute(
                    task_id, prim, &payload,
                ) {
                    Ok(result) => {
                        // If it produced a text output (from Compute/Write), save it to final_answer for CLI compatibility
                        if let Some(text) = result.output.get("result").and_then(|v| v.as_str()) {
                            let answer_path = format!("artifacts/final_answer.{}.txt", task_id);
                            providers::get_filesystem().write(&answer_path, text)?;
                        }

                        // Generic recording of returned artifacts
                        let generation = storage.latest_generation_for_task(task_id)?;
                        for artifact in result.artifacts {
                            storage.append_semantic_artifact(
                                task_id,
                                &step_id,
                                generation,
                                &artifact.artifact_type,
                                &artifact.payload,
                            )?;
                        }

                        // Record the raw primitive result as an evidence artifact
                        storage.append_semantic_artifact(
                            task_id,
                            &step_id,
                            generation,
                            "primitive_result_v1",
                            &result.output,
                        )?;
                    }
                    Err(e) => {
                        let reason = format!("primitive_execution_error: {e}");
                        let _ = worker::fail_step(db, task_id, &worker_id, &step_id, &reason);
                        return Err(anyhow!("step {} failed: {}", step_id, reason));
                    }
                }
            }
        }

        if !executed_via_primitive {
            return Err(anyhow!(
                "step {} does not have a primitive specification",
                step_id
            ));
        }

        worker::complete_step(db, task_id, &worker_id, &step_id)?;
    }

    storage.process_effects_ledger(task_id)?;
    Ok(())
}
