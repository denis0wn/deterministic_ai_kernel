use crate::execution_abi::primitives::{
    ArtifactSpec, PrimitiveKind, PrimitiveResult, PrimitiveSpec,
};
use crate::providers;
use anyhow::Result;
use serde_json::json;

pub struct PrimitiveExecutor;

impl PrimitiveExecutor {
    pub fn execute(
        task_id: &str,
        spec: &PrimitiveSpec,
        task_payload: &str,
    ) -> Result<PrimitiveResult> {
        let payload = &spec.payload;

        match spec.kind {
            PrimitiveKind::Read => {
                let path = payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("repository");

                let content = if path == "repository" {
                    task_payload.to_string()
                } else {
                    providers::get_filesystem().read_to_string(path)?
                };

                Ok(PrimitiveResult {
                    id: spec.id.clone(),
                    status: "ok".to_string(),
                    output: json!({
                        "path": path,
                        "content_len": content.len(),
                        "content": content
                    }),
                    artifacts: vec![],
                })
            }
            PrimitiveKind::Write => {
                let path = payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("artifacts/final_patch.txt");

                let requires_llm = payload
                    .get("requires_llm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let content = if requires_llm {
                    let prompt = format!(
                        "You are a deterministic AI kernel worker.\nTask payload:\n{}\nStep: {}\nExecute write patch step and return only the patch/code.",
                        task_payload, spec.id.0
                    );
                    let llm_res = providers::get_llm().execute_llm(&prompt, None)?;
                    llm_res.text
                } else {
                    payload
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or(task_payload)
                        .to_string()
                };

                providers::get_filesystem().write(path, &content)?;

                Ok(PrimitiveResult {
                    id: spec.id.clone(),
                    status: "ok".to_string(),
                    output: json!({
                        "path": path,
                        "written_bytes": content.len()
                    }),
                    artifacts: vec![],
                })
            }
            PrimitiveKind::Compute => {
                let operation = payload
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("none");
                let detail = payload
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .unwrap_or(task_payload);

                if matches!(operation, "semantic_embedding") {
                    let vector = providers::get_llm().embed_text(detail)?;
                    let artifact_payload = json!({
                        "input_representation": detail,
                        "embedding_dim": vector.len(),
                        "analysis_kind": "semantic_seed"
                    });
                    Ok(PrimitiveResult {
                        id: spec.id.clone(),
                        status: "ok".to_string(),
                        output: json!({
                            "vector_len": vector.len(),
                            "input_representation": detail,
                            "analysis_kind": "semantic_seed"
                        }),
                        artifacts: vec![ArtifactSpec {
                            artifact_type: "analysis_seed".to_string(),
                            payload: artifact_payload,
                        }],
                    })
                } else {
                    let requires_llm = payload
                        .get("requires_llm")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let output_val = if requires_llm {
                        let prompt = format!(
                            "You are a deterministic AI kernel worker.\nTask payload:\n{}\nStep: {}\nExecute this step and return only the result.",
                            task_payload, spec.id.0
                        );
                        let llm_res = providers::get_llm().execute_llm(&prompt, None)?;
                        json!({
                            "result": llm_res.text,
                            "model_name": llm_res.model_name,
                            "model_version": llm_res.model_version
                        })
                    } else {
                        json!({
                            "status": "computed",
                            "task_id": task_id
                        })
                    };

                    Ok(PrimitiveResult {
                        id: spec.id.clone(),
                        status: "ok".to_string(),
                        output: output_val,
                        artifacts: vec![],
                    })
                }
            }
            PrimitiveKind::Route => {
                let requires_llm = payload
                    .get("requires_llm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let route_decision = if requires_llm {
                    let prompt = format!(
                        "You are a deterministic AI kernel verifier.\nTask payload:\n{}\nStep: {}\nVerify and decide: Success or Failure.",
                        task_payload, spec.id.0
                    );
                    let llm_res = providers::get_llm().execute_llm(&prompt, None)?;
                    llm_res.text
                } else {
                    "Success".to_string()
                };

                Ok(PrimitiveResult {
                    id: spec.id.clone(),
                    status: "ok".to_string(),
                    output: json!({
                        "route_decision": route_decision
                    }),
                    artifacts: vec![],
                })
            }
            PrimitiveKind::Wait => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "ok".to_string(),
                output: json!({ "waited": true }),
                artifacts: vec![],
            }),
            PrimitiveKind::Signal => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "ok".to_string(),
                output: json!({ "signalled": true }),
                artifacts: vec![],
            }),
            PrimitiveKind::Spawn => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "ok".to_string(),
                output: json!({ "spawned": true }),
                artifacts: vec![],
            }),
            PrimitiveKind::Complete => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "completed".to_string(),
                output: json!({}),
                artifacts: vec![],
            }),
            PrimitiveKind::Fail => Ok(PrimitiveResult {
                id: spec.id.clone(),
                status: "failed".to_string(),
                output: json!({}),
                artifacts: vec![],
            }),
        }
    }
}
