use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrustLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustContext {
    pub source: String,
    pub trust_level: TrustLevel,
    pub verification_status: String,
    pub policy_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionEvent {
    pub id: String,
    pub task_id: String,
    pub timestamp: String,
    pub event_type: String,
    pub payload: Value,
    pub caused_by: Option<String>,
    pub trust_context: TrustContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateGraphNode {
    pub id: String,
    pub kind: String,
    pub ref_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateGraphEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StateGraph {
    pub nodes: Vec<StateGraphNode>,
    pub edges: Vec<StateGraphEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecisionPoint {
    pub id: String,
    pub timestamp: String,
    pub actor: String,
    pub evidence: Vec<String>,
    pub proposed_action: Value,
    pub selected_action: Value,
    pub confidence: Option<f32>,
    pub trust_context: TrustContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplayCapsule {
    pub capsule_id: String,
    pub execution_id: String,
    pub created_at: String,
    pub state_graph: StateGraph,
    pub event_ids: Vec<String>,
    pub artifacts: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub decision_points: Vec<DecisionPoint>,
    pub determinism_envelope: Value,
    pub trust_context: TrustContext,
}

impl ReplayCapsule {
    #[allow(dead_code)]
    pub fn is_minimally_valid(&self) -> bool {
        !self.capsule_id.is_empty() && !self.execution_id.is_empty() && !self.created_at.is_empty()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.execution_id.trim().is_empty() {
            return Err("execution_id is empty".into());
        }
        if self.capsule_id.trim().is_empty() {
            return Err("capsule_id is empty".into());
        }
        if self.created_at.trim().is_empty() {
            return Err("created_at is empty".into());
        }
        if self.event_ids.is_empty() {
            return Err("event_ids is empty".into());
        }
        if self.state_graph.nodes.is_empty() {
            return Err("state_graph.nodes is empty".into());
        }
        if self.state_graph.nodes.len() < self.event_ids.len() {
            return Err("state_graph.nodes smaller than event_ids".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Modality {
    Text,
    Vision,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIInput {
    Text {
        prompt: String,
    },
    Vision {
        prompt: Option<String>,
        image_refs: Vec<String>,
    },
    Audio {
        prompt: Option<String>,
        audio_refs: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIRequest {
    pub request_id: String,
    pub modality: Modality,
    pub input: AIInput,
    pub model: Option<String>,
    pub deterministic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AIResponse {
    pub request_id: String,
    pub model_id: String,
    pub output_text: String,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AITrace {
    pub model_id: String,
    pub model_hash: String,
    pub prompt_hash: String,
    pub sampling_config: String,
    pub timestamp: u64,
    pub output_hash: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AILifecycleEvent {
    InvocationStarted {
        request_id: String,
        model_id: String,
        trace_id: String,
    },
    ChunkProduced {
        request_id: String,
        sequence: u64,
        content_hash: String,
    },
    InvocationCompleted {
        request_id: String,
        output_hash: String,
        duration_ms: u64,
    },
    InvocationFailed {
        request_id: String,
        error: String,
    },
}
