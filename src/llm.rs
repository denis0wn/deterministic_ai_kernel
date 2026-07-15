use anyhow::{anyhow, Context, Result};

use crate::model_registry::{resolve_model, ModelPurpose};

#[expect(dead_code)]
fn role_for_purpose(purpose: ModelPurpose) -> &'static str {
    match purpose {
        ModelPurpose::CodingAssistant => "coding_assistant",
        ModelPurpose::TaskPlanning => "task_planning",
        ModelPurpose::CodeReview => "code_review",
    }
}

/// Ensure the MLX runtime is up before making an inference call.
/// Silently succeeds if `config/runtime.json` doesn't exist (legacy mode).
fn ensure_runtime_before_call() {
    // Skip in mock/test mode.
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        return;
    }
    match crate::runtime_manager::RuntimeManager::load() {
        Ok(mgr) => {
            if let Err(e) = mgr.ensure_running() {
                eprintln!("[llm] RuntimeManager::ensure_running failed: {e}");
            }
        }
        Err(_) => {
            // No config/runtime.json — legacy mode, user manages the server.
        }
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
    chat_with_model_override(purpose, system_prompt, user_prompt, None).await
}

pub async fn chat_with_model_override(
    purpose: ModelPurpose,
    system_prompt: &str,
    user_prompt: &str,
    model_override: Option<&str>,
) -> Result<String> {
    // Ensure runtime is available before the call.
    ensure_runtime_before_call();

    match do_chat_request(purpose, system_prompt, user_prompt, model_override).await {
        Ok(text) => Ok(text),
        Err(e) => {
            // If it looks like a connection error, try one recovery cycle.
            let msg = e.to_string();
            if msg.contains("Connection refused")
                || msg.contains("connection error")
                || msg.contains("tcp connect error")
                || msg.contains("hyper::Error")
            {
                eprintln!("[llm] Connection error — attempting runtime recovery…");
                if let Ok(mgr) = crate::runtime_manager::RuntimeManager::load() {
                    let _ = mgr.restart();
                }
                // Retry once after recovery.
                do_chat_request(purpose, system_prompt, user_prompt, model_override).await
            } else {
                Err(e)
            }
        }
    }
}

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Inner function that performs the actual TCP native inference request.
async fn do_chat_request(
    purpose: ModelPurpose,
    system_prompt: &str,
    user_prompt: &str,
    model_override: Option<&str>,
) -> Result<String> {
    // Deterministic mock backend for contract tests.
    // Prevents accidental dependency on native inference TCP service.
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        return Ok("analyze current planner flow
validate deterministic cache behavior
add memoization hit assertions"
            .to_string());
    }

    let mut model_name = model_override.unwrap_or("").to_string();
    let mut host = "127.0.0.1".to_string();
    let mut port = 8080u16;

    // Load config dynamically to get host and port
    if let Ok(mgr) = crate::runtime_manager::RuntimeManager::load() {
        host = mgr.config().host.clone();
        port = mgr.config().port;
        if model_name.is_empty() {
            model_name = mgr
                .resolve_runtime_model()
                .unwrap_or_else(|_| mgr.config().default_model.clone());
        }
    }

    if model_name.is_empty() {
        if let Ok(config) = resolve_model(purpose) {
            model_name = config.model;
        }
    }

    let addr = format!("{}:{}", host, port);
    let mut stream = tokio::net::TcpStream::connect(&addr)
        .await
        .with_context(|| format!("Failed to connect to native inference service at {}", addr))?;

    let req = serde_json::json!({
        "method": "generate",
        "params": {
            "model": model_name,
            "messages": [
                {
                    "role": "system",
                    "content": system_prompt
                },
                {
                    "role": "user",
                    "content": user_prompt
                }
            ],
            "temp": 0.0,
            "max_tokens": 2048
        }
    });

    let mut req_str = serde_json::to_string(&req)?;
    req_str.push('\n');

    stream.write_all(req_str.as_bytes()).await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let resp: serde_json::Value = serde_json::from_str(&line)
        .with_context(|| format!("Invalid JSON response from native inference: {}", line))?;

    if resp.get("status").and_then(|v| v.as_str()) == Some("ok") {
        let generated_text = resp
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Response missing 'text' field"))?;
        Ok(generated_text.to_string())
    } else {
        let err = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        Err(anyhow!("Generation failed: {}", err))
    }
}

const CODING_ASSISTANT_SYSTEM_PROMPT: &str = "You are a concise coding assistant.";
const TASK_PLANNER_SYSTEM_PROMPT: &str =
    "You are a concise task planning assistant. Follow output constraints exactly.";

pub async fn chat_with_role(role: &str, system_prompt: &str, user_prompt: &str) -> Result<String> {
    let purpose = match role {
        "coding_assistant" | "coding_fallback" => ModelPurpose::CodingAssistant,
        "task_planning" | "task_planning_fallback" => ModelPurpose::TaskPlanning,
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
    let text = coding_assistant("Reply with exactly one word: OK").await?;
    let normalized = text.trim();

    if normalized.is_empty() {
        return Err(anyhow!("llm smoke mismatch: empty response"));
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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct StructuredResponse {
    pub final_answer: String,
    pub confidence: f64,
}

pub fn clean_structured_json(raw: &str) -> Result<String> {
    let mut temp = raw.to_string();

    // Remove closed thought blocks
    while let Some(start_idx) = temp.find("<thought>") {
        if let Some(end_idx) = temp[start_idx..].find("</thought>") {
            let end_pos = start_idx + end_idx + "</thought>".len();
            temp.replace_range(start_idx..end_pos, "");
        } else {
            // Unclosed, just remove the tag itself
            temp = temp.replace("<thought>", "");
            break;
        }
    }
    temp = temp.replace("</thought>", "");

    // Find the first '{' and the last '}'
    if let Some(first_brace) = temp.find('{') {
        if let Some(last_brace) = temp.rfind('}') {
            if last_brace >= first_brace {
                return Ok(temp[first_brace..=last_brace].to_string());
            }
        }
    }

    Err(anyhow!("No JSON object found in response"))
}

pub async fn chat_structured(
    purpose: ModelPurpose,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<StructuredResponse> {
    let updated_system = format!(
        "{}\n\nCRITICAL: You MUST respond with a JSON object matching this schema: \
         {{\"final_answer\": \"...\", \"confidence\": 0.99}}. \
         Do NOT include any internal reasoning, thought process, explanations, or formatting outside this JSON structure.",
        system_prompt
    );

    let raw_response = chat_with_purpose(purpose, &updated_system, user_prompt).await?;
    let cleaned = clean_structured_json(&raw_response)?;
    let parsed: StructuredResponse = serde_json::from_str(&cleaned)
        .with_context(|| format!("Failed to parse structured JSON: {}", cleaned))?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_structured_json() {
        let input = "<thought>some reasoning here</thought>\n```json\n{\"final_answer\": \"hello\", \"confidence\": 0.95}\n```";
        let cleaned = clean_structured_json(input).unwrap();
        assert_eq!(
            cleaned,
            "{\"final_answer\": \"hello\", \"confidence\": 0.95}"
        );

        let input_unclosed =
            "<thought>unclosed thought {\"final_answer\": \"unclosed\", \"confidence\": 0.8}";
        let cleaned_unclosed = clean_structured_json(input_unclosed).unwrap();
        assert_eq!(
            cleaned_unclosed,
            "{\"final_answer\": \"unclosed\", \"confidence\": 0.8}"
        );
    }
}
