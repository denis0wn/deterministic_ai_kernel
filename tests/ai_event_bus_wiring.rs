use anyhow::Result;
use deterministic_ai_kernel::ai::protocol::{
    AIInput, AIRequest, AIResponse, GenerationConfig, Modality, ResponseChunk, TraceContext,
};
use deterministic_ai_kernel::ai::trace::AITrace;
use deterministic_ai_kernel::event_bus::{stable_event_hash, EventBus};
use deterministic_ai_kernel::kernel_types::AILifecycleEvent;
use deterministic_ai_kernel::llm::record_ai_roundtrip;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_db_path(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir()
        .join(format!("{name}-{nanos}.db"))
        .display()
        .to_string()
}

fn sample_request() -> AIRequest {
    AIRequest {
        request_id: "req-1".to_string(),
        workflow_id: "wf-1".to_string(),
        modality: Modality::Text,
        input: AIInput::Text {
            prompt: "hello".to_string(),
        },
        input_artifacts: Vec::new(),
        prompt: Some("hello".to_string()),
        model: Some("model-x".to_string()),
        generation_config: GenerationConfig::default(),
        trace_context: TraceContext::default(),
        deterministic: true,
    }
}

fn sample_trace() -> AITrace {
    AITrace {
        request_id: "req-1".to_string(),
        workflow_id: "wf-1".to_string(),
        model_id: "model-x".to_string(),
        model_version: None,
        model_hash: stable_event_hash("model-x"),
        prompt_hash: stable_event_hash("hello"),
        input_artifact_hashes: Vec::new(),
        generation_config_hash: stable_event_hash("{}"),
        sampling_config: "temperature_milli=0".to_string(),
        seed: Some(0),
        timestamp: 0,
        output_hash: stable_event_hash("world"),
    }
}

fn sample_response() -> AIResponse {
    AIResponse {
        request_id: "req-1".to_string(),
        model_id: "model-x".to_string(),
        output_text: "world".to_string(),
        finish_reason: Some("stop".to_string()),
        chunks: vec![ResponseChunk {
            sequence: 0,
            text: "world".to_string(),
            done: true,
        }],
        generated_artifacts: Vec::new(),
        execution_metadata: BTreeMap::new(),
    }
}

#[test]
fn ai_lifecycle_roundtrip_wiring_works() -> Result<()> {
    let db = temp_db_path("ai-lifecycle-roundtrip");
    let bus = EventBus::new(&db)?;

    let request = sample_request();
    let response = sample_response();
    let trace = sample_trace();

    record_ai_roundtrip(&bus, "task-1", "step-1", &request, &response, &trace)?;

    let started_payload = serde_json::json!({
        "ai_event": AILifecycleEvent::InvocationStarted {
            request_id: request.request_id.clone(),
            model_id: trace.model_id.clone(),
            trace_id: trace.output_hash.clone(),
        },
        "trace": deterministic_ai_kernel::kernel_types::AITrace {
            model_id: trace.model_id.clone(),
            model_hash: trace.model_hash.clone(),
            prompt_hash: trace.prompt_hash.clone(),
            sampling_config: trace.sampling_config.clone(),
            timestamp: trace.timestamp,
            output_hash: trace.output_hash.clone(),
        },
    });
    let completed_payload = serde_json::json!({
        "ai_event": AILifecycleEvent::InvocationCompleted {
            request_id: request.request_id.clone(),
            output_hash: trace.output_hash.clone(),
            duration_ms: 0,
        },
        "trace": deterministic_ai_kernel::kernel_types::AITrace {
            model_id: trace.model_id.clone(),
            model_hash: trace.model_hash.clone(),
            prompt_hash: trace.prompt_hash.clone(),
            sampling_config: trace.sampling_config.clone(),
            timestamp: trace.timestamp,
            output_hash: trace.output_hash.clone(),
        },
    });

    let started_exec = deterministic_ai_kernel::kernel_types::ExecutionEvent {
        id: "evt-start".to_string(),
        task_id: "task-1".to_string(),
        timestamp: "0".to_string(),
        event_type: "AIInvocationStarted".to_string(),
        payload: started_payload,
        caused_by: None,
        trust_context: deterministic_ai_kernel::kernel_types::TrustContext {
            source: "ai_worker".to_string(),
            trust_level: deterministic_ai_kernel::kernel_types::TrustLevel::Medium,
            verification_status: "recorded".to_string(),
            policy_version: "phase1".to_string(),
        },
    };

    let completed_exec = deterministic_ai_kernel::kernel_types::ExecutionEvent {
        id: "evt-complete".to_string(),
        task_id: "task-1".to_string(),
        timestamp: "0".to_string(),
        event_type: "AIInvocationCompleted".to_string(),
        payload: completed_payload,
        caused_by: None,
        trust_context: deterministic_ai_kernel::kernel_types::TrustContext {
            source: "ai_worker".to_string(),
            trust_level: deterministic_ai_kernel::kernel_types::TrustLevel::Medium,
            verification_status: "recorded".to_string(),
            policy_version: "phase1".to_string(),
        },
    };

    let started = bus.replay_ai_event(&started_exec)?;
    let completed = bus.replay_ai_event(&completed_exec)?;

    match started {
        AILifecycleEvent::InvocationStarted { request_id, .. } => {
            assert_eq!(request_id, "req-1");
        }
        _ => panic!("expected started"),
    }

    match completed {
        AILifecycleEvent::InvocationCompleted { output_hash, .. } => {
            assert_eq!(output_hash, trace.output_hash);
        }
        _ => panic!("expected completed"),
    }

    Ok(())
}
