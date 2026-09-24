use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AITrace {
    pub request_id: String,
    pub workflow_id: String,
    pub model_id: String,
    pub model_version: Option<String>,
    pub model_hash: String,
    pub prompt_hash: String,
    pub input_artifact_hashes: Vec<String>,
    pub generation_config_hash: String,
    pub sampling_config: String,
    pub seed: Option<u64>,
    pub timestamp: u64,
    pub output_hash: String,
}
