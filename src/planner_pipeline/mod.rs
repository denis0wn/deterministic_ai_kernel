use anyhow::Result;
use crate::semantic_bias::BiasVersion;

pub struct PipelineContext {
    pub seed: u64,
    pub bias_version: BiasVersion,
}

pub trait PipelineStage {
    type Input;
    type Output;
    fn run(&self, input: Self::Input, ctx: &PipelineContext) -> Result<Self::Output>;
}

#[derive(Clone)]
pub struct RawInput {
    pub payload: String,
}

#[derive(Clone)]
pub struct IntermediateRepresentation {
    pub steps: Vec<String>,
}

pub struct Plan {
    pub id: String,
    pub steps: Vec<String>,
    pub seed: u64,
}

impl Plan {
    /// Deterministic BLAKE3-based ID from seed + steps
    pub fn new_with_stable_id(seed: u64, steps: Vec<String>) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&seed.to_le_bytes());
        for step in &steps {
            hasher.update(step.as_bytes());
        }
        let id = hasher.finalize().to_hex()[..16].to_string();
        Plan { id, steps, seed }
    }
}

pub mod normalizer;
pub mod parser;
pub mod semantic_mapper;
pub mod critic;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_is_deterministic() {
        let steps = vec!["step one".to_string(), "step two".to_string()];
        let p1 = Plan::new_with_stable_id(42, steps.clone());
        let p2 = Plan::new_with_stable_id(42, steps);
        assert_eq!(p1.id, p2.id);
    }

    #[test]
    fn stable_id_differs_on_different_seed() {
        let steps = vec!["step one".to_string()];
        let p1 = Plan::new_with_stable_id(1, steps.clone());
        let p2 = Plan::new_with_stable_id(2, steps);
        assert_ne!(p1.id, p2.id);
    }

    #[test]
    fn stable_id_is_16_chars() {
        let p = Plan::new_with_stable_id(0, vec!["x".into()]);
        assert_eq!(p.id.len(), 16);
    }
}
