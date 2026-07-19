use anyhow::{anyhow, Result};

use crate::llm::clean_llm_output;
use crate::providers;
use crate::scheduler::{next_ready_step, schedule};
use crate::worker;

fn payload_for_task(task_id: &str) -> Result<String> {
    let path = format!("artifacts/pipeline_input.{}.txt", task_id);
    providers::get_filesystem().read_to_string(&path)
}

fn lower_tool_execution(
    primitive: &crate::execution_abi::primitives::PrimitiveSpec,
) -> Result<crate::execution_abi::primitives::PrimitiveSpec> {
    use crate::execution_abi::primitives::{PrimitiveKind, PrimitiveSpec};

    if primitive.kind != PrimitiveKind::ToolExecution {
        return Ok(primitive.clone());
    }

    let binding = primitive
        .payload
        .get("binding")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("ToolExecution primitive is missing binding"))?;

    let detail = primitive
        .payload
        .get("detail")
        .and_then(|value| value.as_str());
    let materialized = crate::tool_registry::materialize_allowed_tool(binding, detail)?;

    let command = match binding {
        "repo.apply_patch.canonical" => "git diff --check && git diff --binary -- .".to_string(),
        "repo.run_tests.cargo_all_targets" => "cargo test --all-targets".to_string(),
        _ => {
            return Err(anyhow!(
                "ToolExecution binding is not executable in the pipeline runtime: {binding}"
            ));
        }
    };

    Ok(PrimitiveSpec {
        id: primitive.id.clone(),
        kind: PrimitiveKind::Compute,
        payload: serde_json::json!({
            "binding": binding,
            "tool_version": materialized
                .get("tool_version")
                .and_then(|value| value.as_str())
                .unwrap_or("v1"),
            "materialized_tool": materialized,
            "command": command,
        }),
    })
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
    let storage = providers::get_storage();
    storage.set_override_path(Some(db.to_string()));
    let res = execute_effects_inner(db, task_id);
    storage.set_override_path(None);
    res
}

fn execute_effects_inner(db: &str, task_id: &str) -> Result<()> {
    providers::get_filesystem().create_dir_all("artifacts")?;
    let payload = payload_for_task(task_id)?;

    let storage = providers::get_storage();
    let spec = storage.load_exec_spec(task_id)?;

    for step in &spec.steps {
        if let Some(primitive) = &step.primitive {
            if matches!(
                primitive.kind,
                crate::execution_abi::primitives::PrimitiveKind::ToolExecution
            ) {
                let binding = primitive.payload.get("binding").and_then(|v| v.as_str());
                if binding.is_none() || binding == Some("unresolved") {
                    return Err(anyhow!(
                        "pipeline contains unresolved ToolExecution primitive: step_id={};                      a concrete executable primitive binding is required before scheduling",
                        step.step_id
                    ));
                }
            }
        }
    }

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

                let executable_primitive = lower_tool_execution(prim)?;
                match crate::execution::primitive_executor::PrimitiveExecutor::execute(
                    task_id,
                    &executable_primitive,
                    &payload,
                ) {
                    Ok(result) => {
                        let generation = storage.latest_generation_for_task(task_id)?;

                        // If it produced a text output (from Compute/Write), persist final_answer as a semantic artifact.
                        if let Some(text) = result.output.get("result").and_then(|v| v.as_str()) {
                            let cleaned = clean_llm_output(text);
                            storage.append_semantic_artifact(
                                task_id,
                                &step_id,
                                generation,
                                "final_answer",
                                &serde_json::json!({ "text": cleaned }),
                            )?;
                        }

                        // Generic recording of returned artifacts
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
                            "pipeline_step",
                            &result.output,
                        )?;
                    }
                    Err(e) => {
                        let reason = format!("primitive_execution_error: {e}");
                        let _ = worker::fail_step(db, task_id, &worker_id, &step_id, &reason);
                        if reason.contains("retry:") {
                            continue;
                        }
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
