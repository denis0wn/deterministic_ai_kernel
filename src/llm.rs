use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use crate::model_registry::{resolve_model, ModelPurpose};

// ── Global LLM call serialization ────────────────────────────────────────────
// Only one LLM request may be in-flight at a time to prevent GPU OOM from
// concurrent Metal command buffer submissions.

/// Semaphore: max concurrent LLM calls (R7: configurable). Default 1 keeps
/// the historical strictly-sequential behavior; operators may raise it
/// (DAK_LLM_MAX_CONCURRENT) because mlx_lm.server demonstrably serves
/// concurrent requests while actively generating (R7 EXP A).
static LLM_SEMAPHORE: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(llm_max_concurrent()));

/// Shared reqwest client with connection pooling (reuse across calls).
///
/// R4 (HD-3): the request timeout is configurable via
/// `DAK_LLM_REQUEST_TIMEOUT_SECS` (default 120s). A request that consumes
/// the FULL timeout means the endpoint accepted the connection but produced
/// no response — an MLX model/server hang. That condition is now reported
/// with an explicit "TIMED OUT after Ns" message instead of the opaque
/// reqwest "error sending request" wording (see chat_with_purpose).
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(2)
        .pool_idle_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(llm_request_timeout_secs()))
        .build()
        .expect("failed to build HTTP client")
});

/// Effective per-request timeout in seconds (R4). Overridable for tests and
/// operations; clamped to a sane range. R6: with streaming enabled this
/// value is the HARD CAP on total request duration (a truly infinite
/// generation is still bounded); the idle timeout is separate, see
/// llm_idle_timeout_secs(). Default raised 120→300 for the cap role: legit
/// long-but-alive generations must not be killed by the upper bound.
pub fn llm_request_timeout_secs() -> u64 {
    std::env::var("DAK_LLM_REQUEST_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300)
        .clamp(1, 3600)
}

/// R6: idle timeout for STREAMING responses. The request fails only when NO
/// bytes (content chunks, reasoning deltas, server keepalives) arrive for
/// this many seconds. A slow but ALIVE generation — the NEW-1 pattern of
/// long reasoning chains buffered invisibly in non-stream mode — is waited
/// out instead of being misclassified as a hang. Default 45s.
pub fn llm_idle_timeout_secs() -> u64 {
    std::env::var("DAK_LLM_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(45)
        .clamp(1, 3600)
}

/// R6: streaming is the default transport to the MLX endpoint. Set
/// DAK_LLM_STREAMING=off (or 0) to fall back to the legacy non-streaming
/// request (full-response buffering + single total timeout).
pub fn llm_streaming_enabled() -> bool {
    !matches!(
        std::env::var("DAK_LLM_STREAMING").as_deref(),
        Ok("off") | Ok("0")
    )
}

/// R7: max concurrent LLM requests (semaphore capacity, read once at first
/// use). Default 1 = historical sequential behavior; 2-3 validated against
/// mlx_lm.server 0.31.3 (concurrent serving proven, R7 EXP A). Clamped 1..8.
pub fn llm_max_concurrent() -> usize {
    std::env::var("DAK_LLM_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 8)
}

/// R7: how many times an IDLE stall (no chunks for the whole idle window)
/// may be retried on a FRESH connection. Kernel-owned decision, deterministic:
/// a client disconnect frees mlx_lm.server (R7 EXP B), so one abort+retry
/// converts a wedge into recovery. HARD_TIMEOUT_EXCEEDED (chunks alive past
/// the cap) is never retried — an unbounded generation would repeat.
/// Default 1; set DAK_LLM_STALL_RETRIES=0 for strict no-retry semantics.
pub fn llm_stall_retries() -> u64 {
    std::env::var("DAK_LLM_STALL_RETRIES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1)
        .min(3)
}

/// Signature of an idle-stall error (see consume_sse_response) — the only
/// failure class eligible for R7 stall-retry on a fresh connection.
fn is_idle_stall_error(msg: &str) -> bool {
    msg.contains("TIMED OUT after") && msg.contains("waiting for next chunk")
}

// ── R6: OpenAI-compatible SSE streaming ─────────────────────────────────
//
// Security note (session law): the LLM stays untrusted input. The stream
// reader below only ACCUMULATES delta text — nothing from chunk content is
// ever executed, interpreted as commands, or trusted before the existing
// kernel-side validation gates (patch contract, grounding, test gates).

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
}

#[derive(Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    // Reasoning models may deliver the whole answer in `reasoning` (the
    // non-streaming path mirrors this via content.or(reasoning)); live
    // probe B6 showed an empty content stream with the answer only in
    // reasoning deltas. Accumulated separately and used when content is
    // empty — the streaming contract stays identical to non-streaming.
    #[serde(default)]
    reasoning: Option<String>,
}

/// Consume an SSE chat-completions stream. Every received byte chunk resets
/// the idle timer; the accumulated deltas become the answer text under the
/// SAME contract as the non-streaming path: `content`, or `reasoning` when
/// content is empty (reasoning models may emit the answer only in
/// reasoning deltas).
///
/// Errors:
/// - no chunks for `idle_secs` → "... TIMED OUT after {idle}s ..." (idle
///   stall; storage maps this signature to STALL_DETECTED);
/// - total duration exceeds `hard_cap_secs` despite active chunks →
///   "... HARD_TIMEOUT_EXCEEDED after {cap}s ..." (truly unbounded
///   generation; storage maps this signature to HARD_TIMEOUT_EXCEEDED).
async fn consume_sse_response(
    mut response: reqwest::Response,
    idle_secs: u64,
    hard_cap_secs: u64,
) -> Result<String> {
    let idle = Duration::from_secs(idle_secs);
    let hard_cap = Duration::from_secs(hard_cap_secs);
    let start = Instant::now();

    let mut line_buf: Vec<u8> = Vec::new();
    let mut content = String::new();
    let mut reasoning = String::new();

    loop {
        if start.elapsed() > hard_cap {
            return Err(anyhow!(
                "mlx request HARD_TIMEOUT_EXCEEDED after {}s despite active chunks \
                 (generation never terminated)",
                hard_cap_secs
            ));
        }
        let next = tokio::time::timeout(idle, response.chunk()).await;
        match next {
            Err(_) => {
                return Err(anyhow!(
                    "mlx stream TIMED OUT after {}s waiting for next chunk \
                     (no tokens or keepalive received; generation stalled)",
                    idle_secs
                ));
            }
            Ok(Err(e)) => {
                let msg = if e.is_timeout() {
                    format!(
                        "mlx stream TIMED OUT after {}s waiting for next chunk \
                         (no tokens or keepalive received; generation stalled)",
                        idle_secs
                    )
                } else if e.is_connect() {
                    format!("mlx connection failed (endpoint down or unreachable): {e}")
                } else {
                    format!("mlx stream transport error: {e}")
                };
                return Err(anyhow!("{msg}"));
            }
            Ok(Ok(None)) => break, // stream finished
            Ok(Ok(Some(bytes))) => {
                line_buf.extend_from_slice(&bytes);
                // Drain complete lines; keep the partial tail.
                while let Some(pos) = line_buf.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = line_buf.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&line);
                    let line = line.trim();
                    let Some(payload) = line.strip_prefix("data:") else {
                        continue;
                    };
                    let payload = payload.trim();
                    if payload == "[DONE]" {
                        continue;
                    }
                    if let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) {
                        for choice in &chunk.choices {
                            if let Some(delta) = &choice.delta {
                                if let Some(piece) = &delta.content {
                                    content.push_str(piece);
                                }
                                if let Some(piece) = &delta.reasoning {
                                    reasoning.push_str(piece);
                                }
                            }
                        }
                    }
                    // Non-JSON or keepalive payloads: the bytes still
                    // arrived, so the idle timer was already reset — nothing
                    // else to do with them.
                }
            }
        }
    }
    // Contract parity with the non-streaming path: content.or(reasoning).
    // Reasoning models may emit the whole answer in reasoning deltas and
    // leave content empty (live probe B6).
    Ok(if content.is_empty() {
        reasoning
    } else {
        content
    })
}

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

    // R6: streaming by default — the idle timer (not a total-duration
    // timer) decides stalls, so long-but-alive reasoning generations are
    // no longer misclassified as hangs (NEW-1 root cause).
    let streaming = llm_streaming_enabled();
    let mut req_json = serde_json::to_value(&req)?;
    req_json["stream"] = serde_json::Value::Bool(streaming);

    // R7: outer stall-retry loop. An IDLE stall aborts the connection and
    // retries on a FRESH connection (budget: DAK_LLM_STALL_RETRIES, default
    // 1 — a client disconnect frees mlx_lm.server, EXP B). Transport-level
    // retries keep their own budget on every pass; the kernel owns the
    // decision, determinism is preserved (same payload/seed/temperature).
    let stall_budget = llm_stall_retries();
    let mut stall_used = 0u64;
    'request: loop {
        last_err = None;
        for attempt in 0..=MAX_RETRIES {
            let _in_flight_guard = {
                IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
                InFlightGuard
            };
            let result = HTTP_CLIENT
                .post(&url)
                .bearer_auth(&config.api_key)
                .json(&req_json)
                .send()
                .await;
            drop(_in_flight_guard);

            match result {
                Ok(response) => {
                    let status = response.status();

                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
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

                    let text = if streaming {
                        // R6: SSE consumption with idle timeout + hard cap.
                        // R7: an IDLE stall (no chunks at all for the whole
                        // window) aborts this connection and — if budget remains
                        // — retries on a FRESH connection. HARD_TIMEOUT_EXCEEDED
                        // (chunks alive past the cap) is NOT retried: an
                        // unbounded generation would simply repeat.
                        match consume_sse_response(
                            response,
                            llm_idle_timeout_secs(),
                            llm_request_timeout_secs(),
                        )
                        .await
                        {
                            Ok(t) => t,
                            Err(e) => {
                                let msg = e.to_string();
                                if is_idle_stall_error(&msg) && stall_used < stall_budget {
                                    stall_used += 1;
                                    eprintln!(
                                    "[llm] stall-retry {}/{}: idle stall detected, aborting connection and retrying fresh",
                                    stall_used, stall_budget
                                );
                                    continue 'request;
                                }
                                return Err(e);
                            }
                        }
                    } else {
                        let body = response.text().await?;
                        let parsed: ChatResponse = serde_json::from_str(&body)?;
                        parsed
                            .choices
                            .first()
                            .map(|c| {
                                c.message
                                    .content
                                    .clone()
                                    .or(c.message.reasoning.clone())
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default()
                    };

                    // P0 MLX lifecycle: idle timeout counts from the last
                    // COMPLETED inference.
                    crate::mlx_lifecycle::record_activity();
                    return Ok(text);
                }
                Err(e) => {
                    // R4 (HD-3): precise transport-error attribution. A full
                    // client timeout means the endpoint took the connection and
                    // then produced NOTHING for the whole window (model/server
                    // hang). The old opaque "error sending request" message made
                    // these stalls undiagnosable in logs and event evidence.
                    let (err_msg, kind) = if e.is_timeout() {
                        (
                            format!(
                                "mlx request TIMED OUT after {}s waiting for model response \
                             (endpoint accepted the connection but produced no tokens; \
                             model/server hang suspected)",
                                llm_request_timeout_secs()
                            ),
                            LlmErrorKind::Timeout,
                        )
                    } else if e.is_connect() {
                        (
                            format!("mlx connection failed (endpoint down or unreachable): {e}"),
                            LlmErrorKind::ServerOffline,
                        )
                    } else {
                        let msg = format!("mlx request failed: error sending request: {e}");
                        let kind = classify_error(&anyhow::anyhow!("{msg}"));
                        (msg, kind)
                    };
                    // R4 (HD-3): a server that hung for the FULL request timeout
                    // will hang again — retrying only multiplies the silence
                    // (3x 120s). Fast-fail with the diagnostic; transient kinds
                    // (refused/reset/OOM) keep their retry budget.
                    if kind != LlmErrorKind::Timeout && is_retryable(kind) && attempt < MAX_RETRIES
                    {
                        let backoff = backoff_duration(attempt, kind);
                        eprintln!(
                            "[llm] retry {}/{} after {:?} (backoff {:?})",
                            attempt + 1,
                            MAX_RETRIES + 1,
                            kind,
                            backoff
                        );
                        tokio::time::sleep(backoff).await;
                        last_err = Some(anyhow!("{err_msg}"));
                        continue;
                    }
                    return Err(anyhow!("{err_msg}"));
                }
            }
        }

        // Transport retries exhausted on this pass (any stall-retry budget was
        // already spent via `continue 'request`).
        return Err(last_err.unwrap_or_else(|| anyhow!("LLM call failed after retries")));
    }
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

    #[test]
    fn r7_idle_stall_signature_is_precise() {
        // Eligible: the streaming idle stall.
        assert!(is_idle_stall_error(
            "mlx stream TIMED OUT after 45s waiting for next chunk (no tokens or keepalive received; generation stalled)"
        ));
        // NOT eligible: the hard cap (chunks were alive — retry would repeat).
        assert!(!is_idle_stall_error(
            "mlx request HARD_TIMEOUT_EXCEEDED after 300s despite active chunks"
        ));
        // NOT eligible: the legacy non-stream total timeout.
        assert!(!is_idle_stall_error(
            "mlx request TIMED OUT after 120s waiting for model response"
        ));
        // NOT eligible: ordinary transport failures.
        assert!(!is_idle_stall_error(
            "mlx connection failed (endpoint down or unreachable)"
        ));
    }
}
