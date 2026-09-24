use anyhow::{anyhow, Result};

use crate::ai::adapter::AIAdapter;
use crate::ai::mlx_adapter::MlxAdapter;
use crate::kernel_types::{AIModality, AIRequest};
use crate::lm_control;
use crate::model_manifest::system_prompt_for_role;
use crate::model_registry::ModelPurpose;

fn role_for_purpose(purpose: ModelPurpose) -> &'static str {
    match purpose {
        ModelPurpose::CodingAssistant => "coding_assistant",
        ModelPurpose::TaskPlanning => "task_planning",
        ModelPurpose::CodeReview => "code_review",
    }
}

fn build_prompt(system_prompt: &str, user_prompt: &str) -> String {
    format!(
        "SYSTEM:\n{}\n\nUSER:\n{}",
        system_prompt.trim(),
        user_prompt.trim()
    )
}

fn invoke_via_mlx_adapter(role: &str, system_prompt: &str, user_prompt: &str) -> Result<String> {
    lm_control::auto_route(role)?;

    let adapter = MlxAdapter::new();
    let response = adapter.generate(&AIRequest {
        request_id: format!("{}-request", role),
        workflow_id: format!("{}-workflow", role),
        modality: AIModality::Text,
        prompt: build_prompt(system_prompt, user_prompt),
        model_id: Some("gemma4-reasoning".to_string()),
        seed: Some(0),
    })?;

    let text = response.output_text.unwrap_or_default();
    if text.trim().is_empty() {
        return Err(anyhow!(
            "mlx adapter returned empty response for role {}",
            role
        ));
    }

    Ok(text)
}

#[allow(dead_code)]
pub async fn chat(system_prompt: &str, user_prompt: &str) -> Result<String> {
    chat_with_purpose(ModelPurpose::CodingAssistant, system_prompt, user_prompt).await
}

pub async fn chat_with_purpose(
    purpose: ModelPurpose,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String> {
    let role = role_for_purpose(purpose);
    invoke_via_mlx_adapter(role, system_prompt, user_prompt)
}

const CODING_ASSISTANT_SYSTEM_PROMPT: &str = "You are a concise coding assistant.";
const TASK_PLANNER_SYSTEM_PROMPT: &str =
    "You are a concise task planning assistant. Follow output constraints exactly.";

pub async fn chat_with_role(role: &str, system_prompt: &str, user_prompt: &str) -> Result<String> {
    match role {
        "coding_assistant"
        | "coding_fallback"
        | "task_planning"
        | "task_planning_fallback"
        | "code_review" => invoke_via_mlx_adapter(role, system_prompt, user_prompt),
        other => Err(anyhow!("unknown role for llm dispatch: {other}")),
    }
}

pub async fn coding_assistant(user_prompt: &str) -> Result<String> {
    let system_prompt = system_prompt_for_role("coding_assistant")
        .unwrap_or_else(|_| CODING_ASSISTANT_SYSTEM_PROMPT.to_string());
    chat_with_purpose(ModelPurpose::CodingAssistant, &system_prompt, user_prompt).await
}

pub async fn task_planner(user_prompt: &str) -> Result<String> {
    let system_prompt = system_prompt_for_role("task_planning")
        .unwrap_or_else(|_| TASK_PLANNER_SYSTEM_PROMPT.to_string());
    chat_with_purpose(ModelPurpose::TaskPlanning, &system_prompt, user_prompt).await
}

pub async fn smoke() -> Result<()> {
    let adapter = MlxAdapter::new();
    adapter.health_check()?;

    let text = coding_assistant("Reply with exactly: MLX Gemma from Rust works").await?;
    if text.trim().is_empty() {
        return Err(anyhow!("llm smoke mismatch: empty response"));
    }

    println!("LLM_SMOKE_OK");
    println!("MODEL_RESPONSE: {}", text.trim());
    Ok(())
}

pub async fn planner_smoke() -> Result<()> {
    let text = task_planner("Reply with exactly: MLX planner from Rust works").await?;
    if text.trim().is_empty() {
        return Err(anyhow!("planner smoke mismatch: empty response"));
    }

    println!("LLM_PLANNER_SMOKE_OK");
    println!("MODEL_RESPONSE: {}", text.trim());
    Ok(())
}
