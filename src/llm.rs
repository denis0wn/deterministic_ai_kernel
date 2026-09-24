use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

use crate::ai::adapter::{AIAdapter, AdapterMetadata};
use crate::ai::protocol::{
    AIEvent, AIInput, AIRequest, AIResponse, GenerationConfig, Modality, ResponseChunk,
    TraceContext,
};
use crate::ai::router::AIModelRouter;
use crate::ai::trace::AITrace;
use crate::event_bus::{stable_event_hash, EventBus};
use crate::lm_control;
use crate::model_registry::{resolve_model, ModelPurpose};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
}

#[derive(Serialize, Deserialize, Clone)]
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

pub struct AIInvocationFacade;

#[async_trait]
impl AIAdapter for AIInvocationFacade {
    fn metadata(&self) -> AdapterMetadata {
        AdapterMetadata {
            adapter_id: "ai-invocation-facade".to_string(),
            model_id: "router-selected".to_string(),
        }
    }

    async fn invoke(&self, request: &AIRequest) -> Result<AIResponse> {
        Self::invoke(request.clone()).await
    }
}

impl AIInvocationFacade {
    pub async fn invoke(request: AIRequest) -> Result<AIResponse> {
        let adapter = Self;
        let _adapter_meta = adapter.metadata();

        let (model_id, _profile) = AIModelRouter::select_for_request(&request)?;
        let prompt = extract_prompt(&request);

        let trace = AITrace {
            request_id: request.request_id.clone(),
            workflow_id: request.workflow_id.clone(),
            model_id: model_id.clone(),
            model_version: None,
            model_hash: stable_event_hash(&model_id),
            prompt_hash: stable_hash(prompt.as_deref().unwrap_or("")),
            input_artifact_hashes: request
                .input_artifacts
                .iter()
                .map(|a| a.sha256.clone().unwrap_or_else(|| a.artifact_id.clone()))
                .collect(),
            generation_config_hash: hash_generation_config(&request.generation_config),
            sampling_config: format!(
                "temperature_milli={},top_p_milli={:?},max_output_tokens={:?}",
                request.generation_config.temperature_milli,
                request.generation_config.top_p_milli,
                request.generation_config.max_output_tokens
            ),
            seed: request.generation_config.seed,
            timestamp: now_ms(),
            output_hash: String::new(),
        };

        let _started = AIEvent::InvocationStarted {
            request_id: request.request_id.clone(),
            workflow_id: request.workflow_id.clone(),
            model_id: Some(model_id.clone()),
            modality: request.modality.clone(),
        };

        match request.modality {
            Modality::Text => invoke_text(request, model_id, prompt, trace).await,
            Modality::Vision | Modality::Audio => {
                Err(anyhow!("multimodal backend not implemented in phase 1"))
            }
        }
    }
}

fn extract_prompt(request: &AIRequest) -> Option<String> {
    if let Some(prompt) = &request.prompt {
        return Some(prompt.clone());
    }

    match &request.input {
        AIInput::Text { prompt } => Some(prompt.clone()),
        AIInput::Vision { prompt, .. } => prompt.clone(),
        AIInput::Audio { prompt, .. } => prompt.clone(),
    }
}

async fn invoke_text(
    request: AIRequest,
    model_id: String,
    prompt: Option<String>,
    mut trace: AITrace,
) -> Result<AIResponse> {
    let prompt = prompt.unwrap_or_default();
    lm_control::auto_route("task_planning")?;

    let config = resolve_model(ModelPurpose::TaskPlanning)?;
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));

    let req = ChatRequest {
        model: model_id.clone(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: "You are a concise AI worker.".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: prompt.clone(),
            },
        ],
        temperature: (request.generation_config.temperature_milli as f32) / 1000.0,
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
        let _failed = AIEvent::InvocationFailed {
            request_id: request.request_id.clone(),
            error: format!("status={status} body={body}"),
        };

        return Err(anyhow!(
            "llm request failed for model {:?} with status {}: {}",
            model_id,
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

    let chunk = ResponseChunk {
        sequence: 0,
        text: text.clone(),
        done: true,
    };

    let _chunk_event = AIEvent::ChunkProduced {
        request_id: request.request_id.clone(),
        sequence: 0,
        text: text.clone(),
    };

    trace.output_hash = stable_hash(&text);

    let _completed = AIEvent::InvocationCompleted {
        request_id: request.request_id.clone(),
        model_id: model_id.clone(),
        output_hash: Some(trace.output_hash.clone()),
    };

    let mut execution_metadata = BTreeMap::new();
    execution_metadata.insert("adapter_id".to_string(), json!("ai-invocation-facade"));
    execution_metadata.insert("prompt_hash".to_string(), json!(trace.prompt_hash.clone()));
    execution_metadata.insert(
        "generation_config_hash".to_string(),
        json!(trace.generation_config_hash.clone()),
    );
    execution_metadata.insert("output_hash".to_string(), json!(trace.output_hash.clone()));

    Ok(AIResponse {
        request_id: request.request_id,
        model_id,
        output_text: text.clone(),
        finish_reason: Some("stop".to_string()),
        chunks: vec![chunk],
        generated_artifacts: Vec::new(),
        execution_metadata,
    })
}

pub async fn task_planner(user_prompt: &str) -> Result<String> {
    let request = AIRequest {
        request_id: "task-planner-compat".to_string(),
        workflow_id: "workflow-compat".to_string(),
        modality: Modality::Text,
        input: AIInput::Text {
            prompt: user_prompt.to_string(),
        },
        input_artifacts: Vec::new(),
        prompt: Some(user_prompt.to_string()),
        model: None,
        generation_config: GenerationConfig::default(),
        trace_context: TraceContext::default(),
        deterministic: true,
    };

    let adapter = AIInvocationFacade;
    let response = adapter.invoke(&request).await?;
    Ok(response.output_text)
}

fn stable_hash(input: &str) -> String {
    stable_event_hash(input)
}

fn hash_generation_config(cfg: &GenerationConfig) -> String {
    let stable = serde_json::to_string(cfg).unwrap_or_else(|_| "{}".to_string());
    stable_hash(&stable)
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[allow(dead_code)]
pub fn record_ai_roundtrip(
    event_bus: &EventBus,
    task_id: &str,
    _step_id: &str,
    request: &AIRequest,
    _response: &AIResponse,
    trace: &AITrace,
) -> Result<()> {
    let start_event = crate::kernel_types::AILifecycleEvent::InvocationStarted {
        request_id: request.request_id.clone(),
        model_id: trace.model_id.clone(),
        trace_id: trace.output_hash.clone(),
    };
    let complete_event = crate::kernel_types::AILifecycleEvent::InvocationCompleted {
        request_id: request.request_id.clone(),
        output_hash: trace.output_hash.clone(),
        duration_ms: 0,
    };

    let event_trace = crate::kernel_types::AITrace {
        model_id: trace.model_id.clone(),
        model_hash: trace.model_hash.clone(),
        prompt_hash: trace.prompt_hash.clone(),
        sampling_config: trace.sampling_config.clone(),
        timestamp: trace.timestamp,
        output_hash: trace.output_hash.clone(),
    };

    event_bus.record_ai_event(task_id, &start_event, Some(&event_trace))?;
    event_bus.record_ai_event(task_id, &complete_event, Some(&event_trace))?;
    Ok(())
}

pub async fn smoke() -> Result<()> {
    Ok(())
}

pub async fn planner_smoke() -> Result<()> {
    Ok(())
}

pub async fn coding_assistant(prompt: &str) -> Result<String> {
    Ok(prompt.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::trace::AITrace;

    #[test]
    fn adapter_metadata_is_exposed() {
        let adapter = AIInvocationFacade;
        let meta = adapter.metadata();
        assert_eq!(meta.adapter_id, "ai-invocation-facade");
        assert_eq!(meta.model_id, "router-selected");
    }

    #[test]
    fn ai_event_shapes_are_constructible() {
        let started = AIEvent::InvocationStarted {
            request_id: "req-1".into(),
            workflow_id: "wf-1".into(),
            model_id: Some("model-x".into()),
            modality: Modality::Text,
        };
        let chunk = AIEvent::ChunkProduced {
            request_id: "req-1".into(),
            sequence: 0,
            text: "hello".into(),
        };
        let completed = AIEvent::InvocationCompleted {
            request_id: "req-1".into(),
            model_id: "model-x".into(),
            output_hash: Some(stable_event_hash("hello")),
        };

        match started {
            AIEvent::InvocationStarted { workflow_id, .. } => assert_eq!(workflow_id, "wf-1"),
            _ => panic!("unexpected event"),
        }
        match chunk {
            AIEvent::ChunkProduced { sequence, .. } => assert_eq!(sequence, 0),
            _ => panic!("unexpected event"),
        }
        match completed {
            AIEvent::InvocationCompleted { model_id, .. } => assert_eq!(model_id, "model-x"),
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn generation_config_hash_is_stable() {
        let cfg = GenerationConfig::default();
        assert_eq!(hash_generation_config(&cfg), hash_generation_config(&cfg));
    }

    #[test]
    fn ai_trace_output_hash_uses_stable_hash() {
        let mut trace = AITrace {
            request_id: "req-1".into(),
            workflow_id: "wf-1".into(),
            model_id: "model-x".into(),
            model_version: None,
            model_hash: stable_event_hash("model-x"),
            prompt_hash: stable_event_hash("prompt"),
            input_artifact_hashes: vec![],
            generation_config_hash: stable_event_hash("{}"),
            sampling_config: "temperature_milli=0".into(),
            seed: Some(0),
            timestamp: 0,
            output_hash: String::new(),
        };

        trace.output_hash = stable_hash("hello");
        assert_eq!(trace.output_hash, stable_event_hash("hello"));
    }
}
