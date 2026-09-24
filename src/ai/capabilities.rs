use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ModelCapability {
    pub text: bool,
    pub vision: bool,
    pub audio: bool,
    pub code: bool,
    pub reasoning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelModalityProfile {
    pub model_id: String,
    pub capabilities: ModelCapability,
}

impl ModelModalityProfile {
    pub fn supports_text(&self) -> bool {
        self.capabilities.text
    }

    pub fn supports_vision(&self) -> bool {
        self.capabilities.vision
    }

    pub fn supports_audio(&self) -> bool {
        self.capabilities.audio
    }
}
