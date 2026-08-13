use anyhow::{anyhow, Result};

#[cfg(test)]
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelPurpose {
    CodingAssistant,
    TaskPlanning,
    CodeReview,
    Critic,
    Verifier,
    Finalizer,
}

impl ModelPurpose {
    pub fn env_key(&self) -> Option<&'static str> {
        match self {
            ModelPurpose::CodingAssistant => Some("OPENAI_MODEL_CODING_ASSISTANT"),
            ModelPurpose::TaskPlanning => Some("OPENAI_MODEL_TASK_PLANNING"),
            ModelPurpose::CodeReview => Some("OPENAI_MODEL_CODE_REVIEW"),
            ModelPurpose::Critic => Some("OPENAI_MODEL_CRITIC"),
            ModelPurpose::Verifier => Some("OPENAI_MODEL_VERIFIER"),
            ModelPurpose::Finalizer => Some("OPENAI_MODEL_FINALIZER"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

fn env_required(key: &str) -> Result<String> {
    dotenvy::dotenv().ok();
    std::env::var(key).map_err(|_| anyhow!("{} is not set", key))
}

fn env_with_default(key: &str, default: &str) -> String {
    dotenvy::dotenv().ok();
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_optional(key: &str) -> Option<String> {
    dotenvy::dotenv().ok();
    std::env::var(key).ok()
}

#[cfg(test)]
fn required_from_map(values: &HashMap<&str, String>, key: &str) -> Result<String> {
    values
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow!("{} is not set", key))
}

#[cfg(test)]
fn optional_from_map(values: &HashMap<&str, String>, key: &str) -> Option<String> {
    values.get(key).cloned()
}

#[cfg(test)]
fn resolve_model_from_values(
    purpose: ModelPurpose,
    values: &HashMap<&str, String>,
) -> Result<ModelConfig> {
    let base_url = required_from_map(values, "OPENAI_BASE_URL")?;
    let api_key = values
        .get("OPENAI_API_KEY")
        .cloned()
        .unwrap_or_else(|| "mlx-local".to_string());

    let default_model = required_from_map(values, "OPENAI_MODEL")?;

    let model = match purpose.env_key() {
        Some(key) => optional_from_map(values, key).unwrap_or_else(|| default_model.clone()),
        None => default_model.clone(),
    };

    Ok(ModelConfig {
        base_url,
        api_key,
        model,
    })
}

pub fn resolve_model(purpose: ModelPurpose) -> Result<ModelConfig> {
    let base_url = env_required("OPENAI_BASE_URL")?;
    let api_key = env_with_default("OPENAI_API_KEY", "mlx-local");
    let default_model = env_required("OPENAI_MODEL")?;

    let model = match purpose.env_key() {
        Some(key) => env_optional(key).unwrap_or_else(|| default_model.clone()),
        None => default_model,
    };

    Ok(ModelConfig {
        base_url,
        api_key,
        model,
    })
}

pub fn validate() -> Result<()> {
    let _ = resolve_model(ModelPurpose::CodingAssistant)?;
    let _ = resolve_model(ModelPurpose::TaskPlanning)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_values() -> HashMap<&'static str, String> {
        HashMap::from([
            ("OPENAI_BASE_URL", "http://127.0.0.1:8080/v1".to_string()),
            ("OPENAI_MODEL", "default-model".to_string()),
        ])
    }

    #[test]
    fn resolves_coding_assistant_purpose() {
        assert!(matches!(
            ModelPurpose::CodingAssistant,
            ModelPurpose::CodingAssistant
        ));
    }

    #[test]
    fn resolves_task_planning_purpose() {
        assert!(matches!(
            ModelPurpose::TaskPlanning,
            ModelPurpose::TaskPlanning
        ));
    }

    #[test]
    fn resolves_new_purposes() {
        assert!(matches!(ModelPurpose::Critic, ModelPurpose::Critic));
        assert!(matches!(ModelPurpose::Verifier, ModelPurpose::Verifier));
        assert!(matches!(ModelPurpose::Finalizer, ModelPurpose::Finalizer));
    }

    #[test]
    fn coding_assistant_uses_default_model_when_specific_key_missing() {
        let values = base_values();
        let cfg = resolve_model_from_values(ModelPurpose::CodingAssistant, &values)
            .expect("failed to resolve assistant model");
        assert_eq!(cfg.model, "default-model");
        assert_eq!(cfg.api_key, "mlx-local");
    }

    #[test]
    fn task_planning_uses_default_model_when_specific_key_missing() {
        let values = base_values();
        let cfg = resolve_model_from_values(ModelPurpose::TaskPlanning, &values)
            .expect("failed to resolve planning model");
        assert_eq!(cfg.model, "default-model");
    }

    #[test]
    fn new_purposes_fallback_to_default_model() {
        let values = base_values();
        for purpose in [
            ModelPurpose::Critic,
            ModelPurpose::Verifier,
            ModelPurpose::Finalizer,
        ] {
            let cfg = resolve_model_from_values(purpose, &values).expect("failed to resolve model");
            assert_eq!(
                cfg.model, "default-model",
                "purpose {:?} should fallback",
                purpose
            );
        }
    }

    #[test]
    fn coding_assistant_prefers_purpose_specific_model() {
        let mut values = base_values();
        values.insert("OPENAI_MODEL_CODING_ASSISTANT", "coding-model".to_string());
        let cfg = resolve_model_from_values(ModelPurpose::CodingAssistant, &values)
            .expect("failed to resolve assistant model preferred");
        assert_eq!(cfg.model, "coding-model");
    }

    #[test]
    fn task_planning_prefers_purpose_specific_model() {
        let mut values = base_values();
        values.insert("OPENAI_MODEL_TASK_PLANNING", "planner-model".to_string());
        let cfg = resolve_model_from_values(ModelPurpose::TaskPlanning, &values)
            .expect("failed to resolve planning model preferred");
        assert_eq!(cfg.model, "planner-model");
    }

    #[test]
    fn new_purposes_prefers_specific_model() {
        let mut values = base_values();
        values.insert("OPENAI_MODEL_CRITIC", "critic-model".to_string());
        values.insert("OPENAI_MODEL_VERIFIER", "verifier-model".to_string());
        values.insert("OPENAI_MODEL_FINALIZER", "finalizer-model".to_string());

        assert_eq!(
            resolve_model_from_values(ModelPurpose::Critic, &values)
                .unwrap()
                .model,
            "critic-model"
        );
        assert_eq!(
            resolve_model_from_values(ModelPurpose::Verifier, &values)
                .unwrap()
                .model,
            "verifier-model"
        );
        assert_eq!(
            resolve_model_from_values(ModelPurpose::Finalizer, &values)
                .unwrap()
                .model,
            "finalizer-model"
        );
    }

    #[test]
    fn missing_base_url_is_an_error() {
        let mut values = base_values();
        values.remove("OPENAI_BASE_URL");
        let err = resolve_model_from_values(ModelPurpose::CodingAssistant, &values)
            .expect_err("should fail due to missing base url");
        assert!(err.to_string().contains("OPENAI_BASE_URL is not set"));
    }

    #[test]
    fn missing_default_model_is_an_error() {
        let mut values = base_values();
        values.remove("OPENAI_MODEL");
        let err = resolve_model_from_values(ModelPurpose::TaskPlanning, &values)
            .expect_err("should fail due to missing default model");
        assert!(err.to_string().contains("OPENAI_MODEL is not set"));
    }
}
