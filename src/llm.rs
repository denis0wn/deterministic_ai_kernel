use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

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

fn base_url() -> Result<String> {
    dotenvy::dotenv().ok();
    std::env::var("OPENAI_BASE_URL").map_err(|_| anyhow!("OPENAI_BASE_URL is not set"))
}

fn api_key() -> String {
    dotenvy::dotenv().ok();
    std::env::var("OPENAI_API_KEY").unwrap_or_else(|_| "lm-studio".to_string())
}

fn model() -> Result<String> {
    dotenvy::dotenv().ok();
    std::env::var("OPENAI_MODEL").map_err(|_| anyhow!("OPENAI_MODEL is not set"))
}

pub async fn chat(system_prompt: &str, user_prompt: &str) -> Result<String> {
    let url = format!("{}/chat/completions", base_url()?.trim_end_matches('/'));

    let req = ChatRequest {
        model: model()?,
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
    let resp = client
        .post(url)
        .bearer_auth(api_key())
        .json(&req)
        .send()
        .await?
        .error_for_status()?
        .json::<ChatResponse>()
        .await?;

    let text = resp
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    Ok(text)
}

const CODING_ASSISTANT_SYSTEM_PROMPT: &str = "You are a concise coding assistant.";

pub async fn coding_assistant(user_prompt: &str) -> Result<String> {
    chat(CODING_ASSISTANT_SYSTEM_PROMPT, user_prompt).await
}

pub async fn smoke() -> Result<()> {
    let text = coding_assistant("Reply with exactly: LM Studio from Rust works").await?;

    println!("LLM_SMOKE_OK");
    println!("MODEL_RESPONSE: {}", text);
    Ok(())
}
