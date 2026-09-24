use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Modality {
    Text,
    Vision,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactKind {
    Image,
    Audio,
    Document,
    Generated,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub kind: ArtifactKind,
    pub uri: String,
    pub media_type: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIInput {
    Text {
        prompt: String,
    },
    Vision {
        prompt: Option<String>,
        image_refs: Vec<ArtifactRef>,
    },
    Audio {
        prompt: Option<String>,
        audio_refs: Vec<ArtifactRef>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerationConfig {
    pub temperature_milli: u32,
    pub top_p_milli: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub seed: Option<u64>,
    pub stop_sequences: Vec<String>,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            temperature_milli: 0,
            top_p_milli: None,
            max_output_tokens: None,
            seed: Some(0),
            stop_sequences: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceContext {
    pub workflow_id: String,
    pub parent_event_id: Option<String>,
    pub deterministic_replay: bool,
    pub metadata: BTreeMap<String, String>,
}

impl Default for TraceContext {
    fn default() -> Self {
        Self {
            workflow_id: "default-workflow".to_string(),
            parent_event_id: None,
            deterministic_replay: true,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIRequest {
    pub request_id: String,
    pub workflow_id: String,
    pub modality: Modality,
    pub input: AIInput,
    pub input_artifacts: Vec<ArtifactRef>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub generation_config: GenerationConfig,
    pub trace_context: TraceContext,
    pub deterministic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseChunk {
    pub sequence: u32,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIResponse {
    pub request_id: String,
    pub model_id: String,
    pub output_text: String,
    pub finish_reason: Option<String>,
    pub chunks: Vec<ResponseChunk>,
    pub generated_artifacts: Vec<ArtifactRef>,
    pub execution_metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIEvent {
    InvocationStarted {
        request_id: String,
        workflow_id: String,
        model_id: Option<String>,
        modality: Modality,
    },
    ChunkProduced {
        request_id: String,
        sequence: u32,
        text: String,
    },
    InvocationCompleted {
        request_id: String,
        model_id: String,
        output_hash: Option<String>,
    },
    InvocationFailed {
        request_id: String,
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_config_default_is_deterministic_friendly() {
        let cfg = GenerationConfig::default();
        assert_eq!(cfg.temperature_milli, 0);
        assert_eq!(cfg.seed, Some(0));
        assert!(cfg.stop_sequences.is_empty());
    }

    #[test]
    fn text_request_roundtrip_shape_is_stable() {
        let req = AIRequest {
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
        };

        let json = serde_json::to_string(&req).unwrap();
        let decoded: AIRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, req);
    }
}
