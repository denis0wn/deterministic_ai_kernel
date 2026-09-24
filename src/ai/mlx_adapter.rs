use anyhow::Result;

use crate::ai::adapter::{AIAdapter, AICapabilities};
use crate::ai::mlx_runtime::MlxRuntime;
use crate::kernel_types::{AIRequest, AIResponse};

#[derive(Debug, Clone)]
pub struct MlxAdapter {
    runtime: MlxRuntime,
    capabilities: AICapabilities,
}

impl MlxAdapter {
    pub fn new() -> Self {
        Self {
            runtime: MlxRuntime::new(),
            capabilities: AICapabilities {
                text: true,
                reasoning: true,
                code: true,
                vision: false,
                audio: false,
                streaming: false,
            },
        }
    }

    #[allow(dead_code)]
    pub fn smoke_test(&self) -> Result<AIResponse> {
        self.generate(&AIRequest {
            request_id: "mlx-smoke".to_string(),
            workflow_id: "mlx-smoke".to_string(),
            modality: crate::kernel_types::AIModality::Text,
            prompt: "Reply with exactly: MLX Gemma from Rust works".to_string(),
            model_id: Some("gemma4-reasoning".to_string()),
            seed: Some(0),
        })
    }
}

impl Default for MlxAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AIAdapter for MlxAdapter {
    fn capabilities(&self) -> &AICapabilities {
        &self.capabilities
    }

    fn health_check(&self) -> Result<()> {
        self.runtime.health_check()
    }

    fn generate(&self, request: &AIRequest) -> Result<AIResponse> {
        self.runtime.generate(request)
    }
}
