use anyhow::{anyhow, Result};

#[cfg(test)]
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPurpose {
    CodingAssistant,
    TaskPlanning,
    CodeReview,
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
        .unwrap_or_else(|| "lm-studio".to_string());

    let default_model = required_from_map(values, "OPENAI_MODEL")?;

    let model = match purpose {
        ModelPurpose::CodingAssistant => optional_from_map(values, "OPENAI_MODEL_CODING_ASSISTANT")
            .unwrap_or_else(|| default_model.clone()),
        ModelPurpose::TaskPlanning => optional_from_map(values, "OPENAI_MODEL_TASK_PLANNING")
            .unwrap_or_else(|| default_model.clone()),
        ModelPurpose::CodeReview => optional_from_map(values, "OPENAI_MODEL_CODING_ASSISTANT")
            .unwrap_or_else(|| default_model.clone()),
    };

    Ok(ModelConfig {
        base_url,
        api_key,
        model,
    })
}

pub fn resolve_model(purpose: ModelPurpose) -> Result<ModelConfig> {
    let base_url = env_required("OPENAI_BASE_URL")?;
    let api_key = env_with_default("OPENAI_API_KEY", "lm-studio");

    let default_model = env_required("OPENAI_MODEL")?;

    let model =
        match purpose {
            ModelPurpose::CodingAssistant => env_optional("OPENAI_MODEL_CODING_ASSISTANT")
                .unwrap_or_else(|| default_model.clone()),
            ModelPurpose::TaskPlanning => {
                env_optional("OPENAI_MODEL_TASK_PLANNING").unwrap_or_else(|| default_model.clone())
            }
            ModelPurpose::CodeReview => env_optional("OPENAI_MODEL_CODING_ASSISTANT")
                .unwrap_or_else(|| default_model.clone()),
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
            ("OPENAI_BASE_URL", "http://localhost:1234/v1".to_string()),
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
    fn coding_assistant_uses_default_model_when_specific_key_missing() {
        let values = base_values();
        let cfg = resolve_model_from_values(ModelPurpose::CodingAssistant, &values).unwrap();

        assert_eq!(cfg.model, "default-model");
        assert_eq!(cfg.api_key, "lm-studio");
    }

    #[test]
    fn task_planning_uses_default_model_when_specific_key_missing() {
        let values = base_values();
        let cfg = resolve_model_from_values(ModelPurpose::TaskPlanning, &values).unwrap();

        assert_eq!(cfg.model, "default-model");
        assert_eq!(cfg.api_key, "lm-studio");
    }

    #[test]
    fn coding_assistant_prefers_purpose_specific_model() {
        let mut values = base_values();
        values.insert("OPENAI_MODEL_CODING_ASSISTANT", "coding-model".to_string());

        let cfg = resolve_model_from_values(ModelPurpose::CodingAssistant, &values).unwrap();
        assert_eq!(cfg.model, "coding-model");
    }

    #[test]
    fn task_planning_prefers_purpose_specific_model() {
        let mut values = base_values();
        values.insert("OPENAI_MODEL_TASK_PLANNING", "planner-model".to_string());

        let cfg = resolve_model_from_values(ModelPurpose::TaskPlanning, &values).unwrap();
        assert_eq!(cfg.model, "planner-model");
    }

    #[test]
    fn missing_base_url_is_an_error() {
        let mut values = base_values();
        values.remove("OPENAI_BASE_URL");

        let err = resolve_model_from_values(ModelPurpose::CodingAssistant, &values).unwrap_err();
        assert!(err.to_string().contains("OPENAI_BASE_URL is not set"));
    }

    #[test]
    fn missing_default_model_is_an_error() {
        let mut values = base_values();
        values.remove("OPENAI_MODEL");

        let err = resolve_model_from_values(ModelPurpose::TaskPlanning, &values).unwrap_err();
        assert!(err.to_string().contains("OPENAI_MODEL is not set"));
    }
}
