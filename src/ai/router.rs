use anyhow::{anyhow, Result};

use crate::ai::capabilities::ModelModalityProfile;
use crate::ai::protocol::{AIRequest, Modality};
use crate::model_registry::{capability_profile_for_purpose, resolve_model, ModelPurpose};

pub struct AIModelRouter;

impl AIModelRouter {
    pub fn select_for_request(request: &AIRequest) -> Result<(String, ModelModalityProfile)> {
        let purpose = match request.modality {
            Modality::Text => ModelPurpose::TaskPlanning,
            Modality::Vision => ModelPurpose::TaskPlanning,
            Modality::Audio => ModelPurpose::TaskPlanning,
        };

        let model = if let Some(explicit) = request.model.clone() {
            explicit
        } else {
            resolve_model(purpose)?.model
        };

        let profile = capability_profile_for_purpose(purpose)?;

        let supported = match request.modality {
            Modality::Text => profile.supports_text(),
            Modality::Vision => profile.supports_vision(),
            Modality::Audio => profile.supports_audio(),
        };

        if !supported {
            return Err(anyhow!(
                "selected model '{}' does not support requested modality",
                model
            ));
        }

        Ok((model, profile))
    }
}
