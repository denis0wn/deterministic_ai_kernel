use anyhow::Result;
use async_trait::async_trait;

use crate::ai::protocol::{AIRequest, AIResponse};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterMetadata {
    pub adapter_id: String,
    pub model_id: String,
}

#[async_trait]
pub trait AIAdapter: Send + Sync {
    fn metadata(&self) -> AdapterMetadata;
    async fn invoke(&self, request: &AIRequest) -> Result<AIResponse>;
}
