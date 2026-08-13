use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use crate::model_registry::{resolve_model, ModelPurpose};

// ── Global LLM call serialization ────────────────────────────────────────────
// Only one LLM request may be in-flight at a time to prevent GPU OOM from
// concurrent Metal command buffer submissions.

/// Semaphore: max 1 concurrent LLM call.
static LLM_SEMAPHORE: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(1));

/// Shared reqwest client with connection pooling (reuse across calls).
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(2)
        .pool_idle_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build HTTP client")
});

/// Global counter for in-flight LLM requests (observability).
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Guard that decrements IN_FLIGHT on drop (prevents leak on panic).
struct InFlightGuard;

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Error classification for retry decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LlmErrorKind {
    GpuOom,
    ServerOffline,
    ServerAborted,
    Timeout,
    HttpError(u16),
    ParseError,
    Other,
}

fn classify_error(err: &anyhow::Error) -> LlmErrorKind {
    let msg = err.to_string().to_lowercase();
    if msg.contains("insufficient memory")
        || msg.contains("oom")
        || msg.contains("out of memory")
        || msg.contains("kIOGPUCommandBuffer")
    {
        LlmErrorKind::GpuOom
    } else if msg.contains("connection refused") || msg.contains("connection reset") {
        LlmErrorKind::ServerOffline
    } else if msg.contains("broken pipe") || msg.contains("eof") || msg.contains("reset by peer") {
        LlmErrorKind::ServerAborted
    } else if msg.contains("timeout") || msg.contains("timed out") {
        LlmErrorKind::Timeout
    } else if let Some(pos) = msg.find("status ") {
        // Extract the HTTP status code from messages like "with status 503: ..."
        let rest = &msg[pos + 7..];
        let code_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(code) = code_str.parse::<u16>() {
            return LlmErrorKind::HttpError(code);
        }
        LlmErrorKind::Other
    } else if msg.contains("json") || msg.contains("parse") {
        LlmErrorKind::ParseError
    } else {
        LlmErrorKind::Other
    }
}

fn is_retryable(kind: LlmErrorKind) -> bool {
    matches!(
        kind,
        LlmErrorKind::GpuOom
            | LlmErrorKind::ServerOffline
            | LlmErrorKind::ServerAborted
            | LlmErrorKind::Timeout
    )
}

fn backoff_duration(attempt: u32, kind: LlmErrorKind) -> Duration {
    let base_ms = match kind {
        LlmErrorKind::GpuOom => 2000, // GPU OOM needs longer cooldown
        LlmErrorKind::ServerOffline | LlmErrorKind::ServerAborted => 1000,
        LlmErrorKind::Timeout => 500,
        _ => 500,
    };
    // Exponential backoff: base * 2^attempt, capped at 8s
    let ms = (base_ms * 2u32.pow(attempt.min(3))).min(8000);
    Duration::from_millis(ms as u64)
}

const MAX_RETRIES: u32 = 2;

// ── Structs ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String,
    content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

// ── Role-specific system prompts (small-model-optimized) ─────────────────────

pub const PLANNER_PROMPT: &str = r#"You are a task planner for a deterministic execution engine.

RULES:
- Decompose the task into 2-5 concrete steps
- Each step must be one of: AnalyzeTask, PlanExecution, ExecuteChanges, ReadRepository, LocateBug, PatchCode, RunTests, ValidatePatch
- Steps must be in logical execution order
- Be specific. No vague steps like "do stuff"

OUTPUT: Return ONLY a JSON object, no preamble:
{"steps": [{"kind": "StepKind", "detail": "specific description"}]}"#;

pub const EXECUTOR_PROMPT: &str = r#"You are a code executor. Given a specific step, produce the exact output needed.

RULES:
- For AnalyzeTask: return a structured summary of the task
- For PatchCode: return a unified diff in standard format
- For RunTests: return the test command and expected outcome
- For ValidatePatch: return {"verdict": "PASS"} or {"verdict": "FAIL", "reason": "..."}
- Be concise. No explanations unless asked.
- Return ONLY the required output, no preamble.

STEP: {step_kind}
DETAIL: {step_detail}"#;

/// Prompt for AnswerQuestion steps (P0, H-2 fix): interrogative/analytical
/// tasks are answered directly instead of being forced through the
/// code-executor frame. Kept placeholder-free on purpose.
pub const QA_PROMPT: &str = r#"You are a direct question-answering assistant. Answer the question.

RULES:
- Give the direct answer first; add at most 2-3 sentences of reasoning if it helps
- Answer in the same language as the question (Russian question -> Russian answer)
- For arithmetic and analysis, produce the concrete value; do NOT write code unless explicitly asked
- If information is missing, say exactly what is missing; never invent facts
- No code blocks, no diffs, no patch format unless the question explicitly asks for code
- Return ONLY the answer text, no preamble or meta-commentary"#;

/// Prompt for PatchCode steps (P1, H-1 fix): the model must reply with a
/// patch_v1 JSON object grounded in the provided file content. Free-form
/// diffs are no longer accepted; the kernel validates every field.
/// `{target}`, `{content}` and `{task}` are substituted by the executor.
pub const PATCH_PROMPT: &str = r#"You are a precise code-repair assistant. Produce a structured patch.

RULES:
- Use ONLY the file content provided under FILE CONTENT. Never invent code that is not there.
- context_before must be copied EXACTLY (byte-for-byte, including indentation) from FILE CONTENT.
- replacement is the corrected code that replaces context_before; change only the minimal buggy region.
- target_file must be exactly the FILE path given below.
- Do NOT output a unified diff. Do NOT output markdown. Reply with ONE JSON object and nothing else:
{"version":"patch_v1","target_file":"<FILE>","context_before":"<exact bytes from FILE CONTENT>","replacement":"<corrected code>","reason":"<one sentence>"}

FILE: {target}
FILE CONTENT:
<<<
{content}
>>>
TASK: {task}"#;

pub const CRITIC_PROMPT: &str = r#"You are a code quality critic. Review execution results and find defects.

RULES:
- Only report ACTUAL BUGS that would cause incorrect output or runtime errors
- Do NOT flag style preferences, naming conventions, or minor improvements
- Do NOT flag code that works correctly even if you would write it differently
- If the code is correct and produces the right output, return {"pass": true}
- Categories: correctness, completeness, format, logic
- Be specific: file, description, severity
- When in doubt, pass — false positives are worse than missed issues

OUTPUT: Return ONLY JSON, no preamble:
{"pass": true/false, "defects": [{"category": "...", "description": "...", "severity": "high/medium/low"}]}"#;

pub const VERIFIER_PROMPT: &str = r#"You are a verification gate. Check if output matches the expected contract.

RULES:
- Compare output against expected format
- Return PASS if valid, FAIL with reason if not
- Be strict about format compliance
- Do NOT suggest improvements

OUTPUT: Return ONLY JSON, no preamble:
{"verdict": "PASS" | "FAIL", "reason": "..."}"#;

pub const FINALIZER_PROMPT: &str = r#"You are a response synthesizer. Compose a clean final answer from execution results.

RULES:
- Combine results into a coherent answer
- If question: answer directly
- If code changes: summarize what changed
- If errors: explain what failed
- Maximum 200 words
- Return ONLY the answer text, no preamble or meta-commentary"#;

const CODING_ASSISTANT_SYSTEM_PROMPT: &str = "You are a concise coding assistant. Return only code and short explanations. No prose, no fluff.";
const TASK_PLANNER_SYSTEM_PROMPT: &str =
    "You are a concise task planning assistant. Follow output constraints exactly. Return structured output.";

// ── Chat functions ───────────────────────────────────────────────────────────

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
    let mut config = resolve_model(purpose)?;
    if let Some(m) = model_override {
        config.model = m.to_string();
    }
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    // P0 MLX lifecycle: make sure a local model server is actually up
    // before requesting. Starts a managed server when unloaded, reuses the
    // running one otherwise; remote URLs and mock backends pass through.
    // Startup failures surface truthfully (no fake inference possible).
    {
        let base = config.base_url.clone();
        tokio::task::spawn_blocking(move || crate::mlx_lifecycle::ensure_ready(&base))
            .await
            .map_err(|e| anyhow!("lifecycle join failed: {e}"))??;
    }

    let req = ChatRequest {
        model: config.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: Some(system_prompt.to_string()),
                reasoning: None,
            },
            ChatMessage {
                role: "user".to_string(),
                content: Some(user_prompt.to_string()),
                reasoning: None,
            },
        ],
        temperature: 0.0,
        max_tokens: Some(4096),
    };

    let mut last_err = None;
    // INFERENCE state for the lifecycle manager: while this guard lives the
    // idle watcher will never shut the server down (race protection).
    let _lifecycle_guard = crate::mlx_lifecycle::begin_inference();
    // Acquire semaphore ONCE before the retry loop to prevent interleaving
    let _permit = LLM_SEMAPHORE
        .acquire()
        .await
        .map_err(|e| anyhow!("semaphore closed: {e}"))?;

    for attempt in 0..=MAX_RETRIES {
        let _in_flight_guard = {
            IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
            InFlightGuard
        };
        let result = HTTP_CLIENT
            .post(&url)
            .bearer_auth(&config.api_key)
            .json(&req)
            .send()
            .await;
        drop(_in_flight_guard);

        match result {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await?;

                if !status.is_success() {
                    let err = anyhow!(
                        "mlx request failed for model {:?} with status {}: {}",
                        config.model,
                        status,
                        body
                    );
                    let kind = classify_error(&err);
                    if is_retryable(kind) && attempt < MAX_RETRIES {
                        let backoff = backoff_duration(attempt, kind);
                        eprintln!(
                            "[llm] retry {}/{} after {:?} (backoff {:?})",
                            attempt + 1,
                            MAX_RETRIES + 1,
                            kind,
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        last_err = Some(err);
                        continue;
                    }
                    return Err(err);
                }

                let parsed: ChatResponse = serde_json::from_str(&body)?;
                let text = parsed
                    .choices
                    .first()
                    .map(|c| {
                        c.message
                            .content
                            .clone()
                            .or(c.message.reasoning.clone())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();

                // P0 MLX lifecycle: idle timeout counts from the last
                // COMPLETED inference.
                crate::mlx_lifecycle::record_activity();
                return Ok(text);
            }
            Err(e) => {
                let err_msg = format!("{e}");
                let kind = classify_error(&anyhow::anyhow!("{}", err_msg));
                if is_retryable(kind) && attempt < MAX_RETRIES {
                    let backoff = backoff_duration(attempt, kind);
                    eprintln!(
                        "[llm] retry {}/{} after {:?} (backoff {:?})",
                        attempt + 1,
                        MAX_RETRIES + 1,
                        kind,
                        backoff
                    );
                    tokio::time::sleep(backoff).await;
                    last_err = Some(anyhow!("mlx request failed: {err_msg}"));
                    continue;
                }
                return Err(anyhow!("mlx request failed: {err_msg}"));
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("LLM call failed after retries")))
}

/// Structured chat: returns parsed JSON with automatic repair on parse failure.
pub async fn chat_structured(
    purpose: ModelPurpose,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<serde_json::Value> {
    let text = chat_with_purpose(purpose, system_prompt, user_prompt).await?;
    let cleaned = extract_json(&text);

    match serde_json::from_str::<serde_json::Value>(&cleaned) {
        Ok(v) => Ok(v),
        Err(e) => {
            // Retry once with repair prompt
            let repair_prompt = format!(
                "The following response is not valid JSON. Return the SAME answer as valid JSON only.\n\nOriginal response:\n{}",
                text
            );
            let repaired = chat_with_purpose(purpose, system_prompt, &repair_prompt).await?;
            let repaired_cleaned = extract_json(&repaired);
            serde_json::from_str::<serde_json::Value>(&repaired_cleaned)
                .map_err(|e2| anyhow!("JSON parse failed after repair: {} (original: {})", e2, e))
        }
    }
}

/// Extract JSON from text that may contain markdown fences or preamble text.
pub fn extract_json(text: &str) -> String {
    // Try to find JSON block in markdown fences
    if let Some(start) = text.find("```json") {
        let json_start = start + 7;
        if let Some(end) = text[json_start..].find("```") {
            return text[json_start..json_start + end].trim().to_string();
        }
    }
    if let Some(start) = text.find("```") {
        let json_start = start + 3;
        if let Some(end) = text[json_start..].find("```") {
            return text[json_start..json_start + end].trim().to_string();
        }
    }
    // Try to find JSON object/array boundaries
    let obj_start = text.find('{');
    let arr_start = text.find('[');

    match (obj_start, arr_start) {
        // Object appears first — prefer object
        (Some(os), Some(ar)) if os < ar => {
            if let Some(end) = text.rfind('}') {
                return text[os..=end].to_string();
            }
        }
        // Array appears first — prefer array
        (Some(_), Some(ar)) => {
            if let Some(end) = text.rfind(']') {
                return text[ar..=end].to_string();
            }
        }
        // Only object
        (Some(os), None) => {
            if let Some(end) = text.rfind('}') {
                return text[os..=end].to_string();
            }
        }
        // Only array
        (None, Some(ar)) => {
            if let Some(end) = text.rfind(']') {
                return text[ar..=end].to_string();
            }
        }
        _ => {}
    }
    text.trim().to_string()
}

// ── Role dispatchers ─────────────────────────────────────────────────────────

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

// ── Smoke tests ──────────────────────────────────────────────────────────────

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

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_from_markdown_fence() {
        let text = "Here is the result:\n```json\n{\"key\": \"value\"}\n```";
        let result = extract_json(text);
        assert_eq!(result, r#"{"key": "value"}"#);
    }

    #[test]
    fn extract_json_from_bare_text() {
        let text = "The answer is {\"key\": \"value\"} done.";
        let result = extract_json(text);
        assert_eq!(result, r#"{"key": "value"}"#);
    }

    #[test]
    fn extract_json_array() {
        let text = "Steps: [{\"kind\": \"Step1\"}, {\"kind\": \"Step2\"}]";
        let result = extract_json(text);
        assert_eq!(result, r#"[{"kind": "Step1"}, {"kind": "Step2"}]"#);
    }

    #[test]
    fn extract_json_passthrough() {
        let text = r#"{"valid": true}"#;
        let result = extract_json(text);
        assert_eq!(result, r#"{"valid": true}"#);
    }

    #[test]
    fn new_prompts_are_not_empty() {
        assert!(!PLANNER_PROMPT.is_empty());
        assert!(!EXECUTOR_PROMPT.is_empty());
        assert!(!QA_PROMPT.is_empty());
        assert!(!PATCH_PROMPT.is_empty());
        assert!(!CRITIC_PROMPT.is_empty());
        assert!(!VERIFIER_PROMPT.is_empty());
        assert!(!FINALIZER_PROMPT.is_empty());
    }

    #[test]
    fn classify_gpu_oom() {
        let err = anyhow!("Command buffer execution failed: Insufficient Memory");
        assert_eq!(classify_error(&err), LlmErrorKind::GpuOom);
    }

    #[test]
    fn classify_server_offline() {
        let err = anyhow!("connection refused");
        assert_eq!(classify_error(&err), LlmErrorKind::ServerOffline);
    }

    #[test]
    fn classify_timeout() {
        let err = anyhow!("request timed out");
        assert_eq!(classify_error(&err), LlmErrorKind::Timeout);
    }

    #[test]
    fn classify_http_error() {
        let err = anyhow!("mlx request failed with status 503: busy");
        assert_eq!(classify_error(&err), LlmErrorKind::HttpError(503));
    }

    #[test]
    fn classify_parse_error() {
        let err = anyhow!("JSON parse error at line 1");
        assert_eq!(classify_error(&err), LlmErrorKind::ParseError);
    }

    #[test]
    fn retryable_errors() {
        assert!(is_retryable(LlmErrorKind::GpuOom));
        assert!(is_retryable(LlmErrorKind::ServerOffline));
        assert!(is_retryable(LlmErrorKind::ServerAborted));
        assert!(is_retryable(LlmErrorKind::Timeout));
        assert!(!is_retryable(LlmErrorKind::ParseError));
        assert!(!is_retryable(LlmErrorKind::Other));
    }

    #[test]
    fn backoff_increases_with_attempt() {
        let d0 = backoff_duration(0, LlmErrorKind::GpuOom);
        let d1 = backoff_duration(1, LlmErrorKind::GpuOom);
        let d2 = backoff_duration(2, LlmErrorKind::GpuOom);
        assert!(d0 < d1);
        assert!(d1 < d2);
        assert!(d2 <= Duration::from_secs(8)); // capped
    }

    #[test]
    fn semaphore_exists() {
        // Just verify the lazy static is initialized
        let _ = &*LLM_SEMAPHORE;
    }

    #[test]
    fn in_flight_counter_starts_zero() {
        assert_eq!(IN_FLIGHT.load(Ordering::Relaxed), 0);
    }
}
