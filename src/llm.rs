use anyhow::{anyhow, Context, Result};
use std::sync::atomic::{AtomicUsize, Ordering};

pub static PROMPT_TOKENS: AtomicUsize = AtomicUsize::new(0);
pub static COMPLETION_TOKENS: AtomicUsize = AtomicUsize::new(0);
pub static REQUEST_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub request_count: usize,
}

pub fn get_llm_usage() -> Option<LlmUsage> {
    let count = REQUEST_COUNT.load(Ordering::Relaxed);
    if count == 0 {
        None
    } else {
        Some(LlmUsage {
            prompt_tokens: PROMPT_TOKENS.load(Ordering::Relaxed),
            completion_tokens: COMPLETION_TOKENS.load(Ordering::Relaxed),
            request_count: count,
        })
    }
}

pub fn reset_llm_usage() {
    PROMPT_TOKENS.store(0, Ordering::Relaxed);
    COMPLETION_TOKENS.store(0, Ordering::Relaxed);
    REQUEST_COUNT.store(0, Ordering::Relaxed);
}

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
    REQUEST_COUNT.fetch_add(1, Ordering::Relaxed);
    // Deterministic mock backend for contract tests.
    // Prevents accidental dependency on native inference TCP service.
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        return Ok("write artifacts/bench_fact.txt with hello_kernel\nrun tests\nread artifacts/bench_fact.txt"
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
        if let Some(usage) = resp.get("usage") {
            if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                PROMPT_TOKENS.fetch_add(prompt as usize, Ordering::Relaxed);
            }
            if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                COMPLETION_TOKENS.fetch_add(completion as usize, Ordering::Relaxed);
            }
        }
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
const MATH_SOLVER_SYSTEM_PROMPT: &str =
    "You are solving a mathematical problem. Work carefully. Before answering: 1. Define the exact success criterion. 2. Derive the formula or argument step by step. 3. Re-check the derivation independently. 4. Test the result on a smaller analogous case when possible. 5. If you are not certain, say so explicitly. Return the full solution draft in plain text. Do not guess and do not jump straight to a short final answer.";

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

pub async fn math_solver(user_prompt: &str) -> Result<String> {
    chat_with_purpose(
        ModelPurpose::CodingAssistant,
        MATH_SOLVER_SYSTEM_PROMPT,
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

/// Strip model control / channel / thought wrappers; keep final content only.
pub fn clean_llm_output(raw: &str) -> String {
    let mut s = raw.to_string();

    // Gemma / chat-template style channel markers
    for tag in [
        "<|channel>thought",
        "<|channel|>",
        "<channel|>",
        "<|channel>",
        "</channel>",
    ] {
        s = s.replace(tag, "");
    }

    // Closed <thought>...</thought>
    while let Some(start_idx) = s.find("<thought>") {
        if let Some(end_idx) = s[start_idx..].find("</thought>") {
            let end_pos = start_idx + end_idx + "</thought>".len();
            s.replace_range(start_idx..end_pos, "");
        } else {
            s = s.replace("<thought>", "");
            break;
        }
    }
    s = s.replace("</thought>", "");

    s = s
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();

    // Prefer content after an explicit answer marker if present
    for marker in [
        "final:",
        "Final:",
        "FINAL:",
        "answer:",
        "Answer:",
        "ANSWER:",
        "here's the answer:",
        "Here's the answer:",
        "Here is the answer:",
    ] {
        if let Some(i) = s.find(marker) {
            s = s[i + marker.len()..].trim().to_string();
            break;
        }
    }

    // Strip leading boilerplate labels linearly until stable
    loop {
        let trimmed = s.trim_start();
        let next = if let Some(rest) = trimmed.strip_prefix("Goal:") {
            rest.trim_start().to_string()
        } else if let Some(rest) = trimmed.strip_prefix("Plan:") {
            rest.trim_start().to_string()
        } else if let Some(rest) = trimmed.strip_prefix("Summary:") {
            rest.trim_start().to_string()
        } else if let Some(rest) = trimmed.strip_prefix("Certainly!") {
            rest.trim_start().to_string()
        } else if let Some(rest) = trimmed.strip_prefix("Sure!") {
            rest.trim_start().to_string()
        } else {
            break;
        };

        if next == s {
            break;
        }
        s = next;
    }

    // If the model emits multiple sentences and the last sentence is the shortest
    // factual tail, prefer that tail over earlier instructional boilerplate.
    let sentence_parts = s
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if sentence_parts.len() >= 2 {
        if let Some(last) = sentence_parts.last() {
            let first = sentence_parts.first().copied().unwrap_or("");
            let last_lower = last.to_ascii_lowercase();
            let first_lower = first.to_ascii_lowercase();
            let looks_instructional = first_lower.contains("return exactly")
                || first_lower.contains("nothing else")
                || first_lower.starts_with("goal:")
                || first_lower.starts_with("plan:")
                || first_lower.starts_with("summary:");
            let looks_compact_final = !last_lower.contains("plan:")
                && !last_lower.contains("goal:")
                && !last_lower.contains("summary:")
                && last.split_whitespace().count() <= 12;

            if looks_instructional && looks_compact_final {
                s = last.trim().to_string();
            }
        }
    }

    s
}

pub fn clean_structured_json(raw: &str) -> Result<String> {
    let mut temp = clean_llm_output(raw);

    // Remove any residual thought wrappers after channel strip
    while let Some(start_idx) = temp.find("<thought>") {
        if let Some(end_idx) = temp[start_idx..].find("</thought>") {
            let end_pos = start_idx + end_idx + "</thought>".len();
            temp.replace_range(start_idx..end_pos, "");
        } else {
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

pub async fn paraphrase_summary(
    summary_json: &str,
    warning_message: Option<&str>,
) -> Result<String> {
    let system_prompt = if let Some(warning) = warning_message {
        format!(
            "You are a concise summarizer. You will receive a JSON representing the deterministic execution summary of a pipeline run. \
             Your task is to paraphrase it into a short, user-friendly human-readable description. \
             CRITICAL RULES: \
             1. Do NOT add new findings or conclusions. \
             2. Do NOT change the verdict/status of the workflow. \
             3. Return exactly one sentence. \
             4. Do NOT explain your reasoning. \
             5. Do NOT prepend labels or introductory text such as 'Goal:', 'Plan:', 'Summary:', 'Answer:', or similar boilerplate. \
             6. Output only the final human-readable answer text. \
             7. Absolutely do NOT contradict the status or replay validation result. \
             WARNING: {}",
            warning
        )
    } else {
        "You are a concise summarizer. You will receive a JSON representing the deterministic execution summary of a pipeline run. \
         Your task is to paraphrase it into a short, user-friendly human-readable description. \
         CRITICAL RULES: \
         1. Do NOT add new findings or conclusions. \
         2. Do NOT change the verdict/status of the workflow. \
         3. Return exactly one sentence. \
         4. Do NOT explain your reasoning. \
         5. Do NOT prepend labels or introductory text such as 'Goal:', 'Plan:', 'Summary:', 'Answer:', or similar boilerplate. \
         6. Output only the final human-readable answer text. \
         7. Absolutely do NOT contradict the status or replay validation result.".to_string()
    };

    let user_prompt = format!("Execution Summary:\n{}", summary_json);

    let response = if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        if summary_json.contains("\"status\": \"success/committed\"")
            && summary_json.contains("FORCE_CONTRADICTION")
        {
            "The workflow failed despite success status.".to_string()
        } else {
            let replay_status = if summary_json.contains("\"replay_valid\": true") {
                "valid"
            } else {
                "invalid"
            };
            let status_desc = if summary_json.contains("\"status\": \"success/committed\"") {
                "completed successfully"
            } else {
                "failed"
            };
            format!(
                "Workflow {} with completed steps. Replay is {}, and there were retries.",
                status_desc, replay_status
            )
        }
    } else {
        chat_with_purpose(ModelPurpose::CodingAssistant, &system_prompt, &user_prompt).await?
    };

    Ok(clean_llm_output(&response))
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

    #[test]
    fn test_clean_llm_output_strips_channel_thought() {
        let raw = "<|channel>thought internal plan here <channel|> final content only";
        let cleaned = clean_llm_output(raw);
        assert!(!cleaned.contains("<|channel>thought"));
        assert!(!cleaned.contains("<channel|>"));
        assert!(cleaned.contains("final content only"));
    }
}
