use serde::{Deserialize, Serialize};

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

pub mod primitives;
