use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::kernel_types::{AIRequest, AIResponse};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AICapabilities {
    pub text: bool,
    pub reasoning: bool,
    pub code: bool,
    pub vision: bool,
    pub audio: bool,
    pub streaming: bool,
}

#[allow(dead_code)]
pub trait AIAdapter: Send + Sync {
    fn capabilities(&self) -> &AICapabilities;
    fn generate(&self, request: &AIRequest) -> Result<AIResponse>;
    fn health_check(&self) -> Result<()>;
}
