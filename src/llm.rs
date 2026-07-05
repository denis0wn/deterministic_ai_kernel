use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::lm_control;
use crate::model_registry::{resolve_model, ModelPurpose};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

fn role_for_purpose(purpose: ModelPurpose) -> &'static str {
    match purpose {
        ModelPurpose::CodingAssistant => "coding_assistant",
        ModelPurpose::TaskPlanning => "task_planning",
        ModelPurpose::CodeReview => "code_review",
    }
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
    lm_control::auto_route(role)?;

    let config = resolve_model(purpose)?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let req = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            },
        ],
        temperature: 0.0,
    };

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .bearer_auth(&config.api_key)
        .json(&req)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(anyhow!(
            "lm studio request failed for model {:?} with status {}: {}",
            config.model,
            status,
            body
        ));
    }

    let parsed: ChatResponse = serde_json::from_str(&body)?;
    let text = parsed
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    Ok(text)
}

const CODING_ASSISTANT_SYSTEM_PROMPT: &str = "You are a concise coding assistant.";
const TASK_PLANNER_SYSTEM_PROMPT: &str =
    "You are a concise task planning assistant. Follow output constraints exactly.";


pub async fn chat_with_role(role: &str, system_prompt: &str, user_prompt: &str) -> Result<String> {
    let purpose = match role {
        "coding_assistant" | "coding_fallback" => ModelPurpose::CodingAssistant,
        "task_planning"    | "task_planning_fallback" => ModelPurpose::TaskPlanning,
        "code_review" => ModelPurpose::CodeReview,
        other => return Err(anyhow::anyhow!("unknown role for llm dispatch: {other}")),
    };
    chat_with_purpose(purpose, system_prompt, user_prompt).await
}

pub async fn coding_assistant(user_prompt: &str) -> Result<String> {
    chat_with_purpose(
        ModelPurpose::CodingAssistant,
        CODING_ASSISTANT_SYSTEM_PROMPT,
        user_prompt,
    )
    .await
}

pub async fn task_planner(user_prompt: &str) -> Result<String> {
    chat_with_purpose(
        ModelPurpose::TaskPlanning,
        TASK_PLANNER_SYSTEM_PROMPT,
        user_prompt,
    )
    .await
}

pub async fn smoke() -> Result<()> {
    let expected = "LM Studio from Rust works";
    let text = coding_assistant("Reply with exactly: LM Studio from Rust works").await?;
    let normalized = text.trim();

    if normalized.is_empty() {
        return Err(anyhow!("llm smoke mismatch: empty response"));
    }

    if !normalized.contains("LM Studio from Rust") {
        return Err(anyhow!(
            "llm smoke mismatch: expected response to mention {:?}, got {:?}",
            expected,
            normalized
        ));
    }

    println!("LLM_SMOKE_OK");
    println!("MODEL_RESPONSE: {}", normalized);
    Ok(())
}

pub async fn planner_smoke() -> Result<()> {
    let expected = "Task planner from Rust works";
    let text = task_planner("Reply with exactly: Task planner from Rust works").await?;

    if text.trim() != expected {
        return Err(anyhow!(
            "planner smoke mismatch: expected {:?}, got {:?}",
            expected,
            text.trim()
        ));
    }

    println!("LLM_PLANNER_SMOKE_OK");
    println!("MODEL_RESPONSE: {}", text.trim());
    Ok(())
}
