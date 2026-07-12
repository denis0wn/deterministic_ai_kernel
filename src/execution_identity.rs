use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExecutionId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimitiveId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SpecVersion(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityId(pub String);

impl ExecutionId {
    pub fn new(val: &str) -> Self {
        Self(val.to_string())
    }

    pub fn stable_hash(&self) -> String {
        let bytes = self.0.as_bytes();
        let hash = blake3::hash(bytes);
        hash.to_hex()[..16].to_string()
    }
}

impl PrimitiveId {
    pub fn new(val: &str) -> Self {
        Self(val.to_string())
    }
}

impl SpecVersion {
    pub fn new(val: u32) -> Self {
        Self(val)
    }
}

impl CapabilityId {
    pub fn new(val: &str) -> Self {
        Self(val.to_string())
    }
}
