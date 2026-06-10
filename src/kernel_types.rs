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
        !self.capsule_id.is_empty()
            && !self.execution_id.is_empty()
            && !self.created_at.is_empty()
    }
}
