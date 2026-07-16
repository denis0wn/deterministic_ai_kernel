//! Plan Generator trait and implementations.
//!
//! This module provides dependency injection for plan generation backends,
//! replacing the previous global `DAK_LM_BACKEND` environment variable approach.
//!
//! # Architecture
//! - `PlanGenerator` trait: abstract interface for generating execution steps
//! - `LocalParserGenerator`: deterministic local parser (default for tests/replay)
//! - `MlxGenerator`: external MLX server backend (production use)
//! - `CompositeGenerator`: tries MLX first, falls back to local parser

use anyhow::Result;

/// Abstract interface for generating execution steps from a payload.
///
/// Implementations must be deterministic when given the same input.
/// Replay and tests should use `LocalParserGenerator` for guaranteed determinism.
pub trait PlanGenerator: Send + Sync {
    /// Generate execution steps from a natural language payload.
    ///
    /// Returns `Ok(Some(steps))` if the generator produced steps,
    /// `Ok(None)` if the generator cannot handle this payload (fallback),
    /// or `Err` on unrecoverable failure.
    fn generate_steps(&self, payload: &str) -> Result<Option<Vec<String>>>;

    /// Human-readable name for observability/logging.
    fn name(&self) -> &'static str;
}

// ── Local Parser Generator ───────────────────────────────────────────────────

/// Deterministic local parser that converts structured text into steps.
/// Always produces the same output for the same input.
/// Use this for replay, tests, and offline operation.
pub struct LocalParserGenerator;

impl PlanGenerator for LocalParserGenerator {
    fn generate_steps(&self, _payload: &str) -> Result<Option<Vec<String>>> {
        // Return None to signal fallback to Pipeline's built-in parser stages
        // (Normalizer → Parser → SemanticMapper). The Pipeline handles this.
        Ok(None)
    }

    fn name(&self) -> &'static str {
        "local_parser"
    }
}

// ── MLX Generator ────────────────────────────────────────────────────────────

/// External MLX server backend for LLM-based plan generation.
/// Non-deterministic by nature — use only in production, never in replay.
pub struct MlxGenerator;

impl PlanGenerator for MlxGenerator {
    fn generate_steps(&self, payload: &str) -> Result<Option<Vec<String>>> {
        Ok(query_mlx_server(payload))
    }

    fn name(&self) -> &'static str {
        "mlx_server"
    }
}

fn query_mlx_server(payload: &str) -> Option<Vec<String>> {
    // Resolve model config
    let config =
        crate::model_registry::resolve_model(crate::model_registry::ModelPurpose::TaskPlanning)
            .ok()?;

    // Build blocking client with a short timeout
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;

    // Test reachability (GET base_url/models)
    let models_url = format!("{}/models", config.base_url.trim_end_matches('/'));
    client.get(&models_url).send().ok()?;

    // Send completions request
    let completions_url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let payload_json = serde_json::json!({
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": "You are a concise task planning assistant. Follow output constraints exactly. Return one step per line without numbering or bullet points."
            },
            {
                "role": "user",
                "content": format!("Generate short task execution steps for: {}", payload)
            }
        ],
        "temperature": 0.0
    });

    let res = client
        .post(completions_url)
        .bearer_auth(&config.api_key)
        .json(&payload_json)
        .send()
        .ok()?;

    if !res.status().is_success() {
        return None;
    }

    // Parse response
    #[derive(serde::Deserialize)]
    struct Choice {
        message: Message,
    }
    #[derive(serde::Deserialize)]
    struct Message {
        content: String,
    }
    #[derive(serde::Deserialize)]
    struct Response {
        choices: Vec<Choice>,
    }

    let body = res.json::<Response>().ok()?;
    let text = body.choices.first()?.message.content.clone();

    let mut steps = Vec::new();
    for line in text.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches(|c: char| {
                c.is_ascii_digit() || c == '.' || c == '-' || c == ')' || c.is_whitespace()
            })
            .trim()
            .trim_end_matches('.')
            .to_string();
        if !trimmed.is_empty() {
            steps.push(trimmed);
        }
    }

    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
}

// ── Composite Generator ──────────────────────────────────────────────────────

/// Tries primary generator first, falls back to secondary on None or Err.
/// Production default: MlxGenerator → LocalParserGenerator.
pub struct CompositeGenerator {
    primary: Box<dyn PlanGenerator>,
    fallback: Box<dyn PlanGenerator>,
}

impl CompositeGenerator {
    pub fn new(primary: Box<dyn PlanGenerator>, fallback: Box<dyn PlanGenerator>) -> Self {
        Self { primary, fallback }
    }

    /// Production default: MLX → Local Parser
    pub fn production_default() -> Self {
        Self::new(
            Box::new(MlxGenerator),
            Box::new(LocalParserGenerator),
        )
    }
}

impl PlanGenerator for CompositeGenerator {
    fn generate_steps(&self, payload: &str) -> Result<Option<Vec<String>>> {
        match self.primary.generate_steps(payload) {
            Ok(Some(steps)) => Ok(Some(steps)),
            Ok(None) => self.fallback.generate_steps(payload),
            Err(_) => self.fallback.generate_steps(payload),
        }
    }

    fn name(&self) -> &'static str {
        "composite"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_parser_returns_none_for_fallback() {
        let gen = LocalParserGenerator;
        let result = gen.generate_steps("test payload").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn composite_falls_back_on_none() {
        struct AlwaysNone;
        impl PlanGenerator for AlwaysNone {
            fn generate_steps(&self, _: &str) -> Result<Option<Vec<String>>> {
                Ok(None)
            }
            fn name(&self) -> &'static str { "always_none" }
        }

        struct AlwaysSome;
        impl PlanGenerator for AlwaysSome {
            fn generate_steps(&self, _: &str) -> Result<Option<Vec<String>>> {
                Ok(Some(vec!["step1".into()]))
            }
            fn name(&self) -> &'static str { "always_some" }
        }

        let composite = CompositeGenerator::new(
            Box::new(AlwaysNone),
            Box::new(AlwaysSome),
        );
        let result = composite.generate_steps("test").unwrap().unwrap();
        assert_eq!(result, vec!["step1"]);
    }

    #[test]
    fn composite_uses_primary_when_available() {
        struct PrimaryGen;
        impl PlanGenerator for PrimaryGen {
            fn generate_steps(&self, _: &str) -> Result<Option<Vec<String>>> {
                Ok(Some(vec!["primary_step".into()]))
            }
            fn name(&self) -> &'static str { "primary" }
        }

        struct FallbackGen;
        impl PlanGenerator for FallbackGen {
            fn generate_steps(&self, _: &str) -> Result<Option<Vec<String>>> {
                Ok(Some(vec!["fallback_step".into()]))
            }
            fn name(&self) -> &'static str { "fallback" }
        }

        let composite = CompositeGenerator::new(
            Box::new(PrimaryGen),
            Box::new(FallbackGen),
        );
        let result = composite.generate_steps("test").unwrap().unwrap();
        assert_eq!(result, vec!["primary_step"]);
    }
}
