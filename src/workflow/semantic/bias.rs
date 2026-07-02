use std::collections::BTreeMap;

use crate::workflow::contract::StepKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BiasVersion {
    V1,
}

impl std::fmt::Display for BiasVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BiasVersion::V1 => write!(f, "v1"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticBias {
    pub version: BiasVersion,
    pub seed: u64,
    pub preferred: Vec<StepKind>,
    pub weights: BTreeMap<String, f64>,
}

impl SemanticBias {
    pub fn metadata(&self) -> BiasMetadata {
        BiasMetadata {
            version: self.version,
            seed: self.seed,
            preferred_count: self.preferred.len(),
            weighted_count: self.weights.len(),
        }
    }

    pub fn explain_lines(&self) -> Vec<String> {
        let meta = self.metadata();
        let mut lines = vec![
            format!("bias.version={}", self.version),
            format!("bias.seed={}", self.seed),
            format!("bias.preferred={:?}", self.preferred),
            format!("bias.meta.version={}", meta.version),
            format!("bias.meta.seed={}", meta.seed),
            format!("bias.meta.preferred_count={}", meta.preferred_count),
            format!("bias.meta.weighted_count={}", meta.weighted_count),
        ];

        for (kind, weight) in &self.weights {
            lines.push(format!("bias.weight.{kind}={weight:.6}"));
        }

        lines
    }

    pub fn neutral_for(domain: &[StepKind]) -> Self {
        let preferred = domain.to_vec();
        let weights = domain.iter().map(|k| (format!("{:?}", k), 1.0)).collect();
        Self {
            version: BiasVersion::V1,
            seed: 0,
            preferred,
            weights,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiasMetadata {
    pub version: BiasVersion,
    pub seed: u64,
    pub preferred_count: usize,
    pub weighted_count: usize,
}
