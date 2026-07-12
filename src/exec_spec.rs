use crate::execution_abi::primitives::PrimitiveSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub step_id: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Constraint {
    pub target: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Policy {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRequirement {
    pub artifact_type: String,
    pub schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepSpec {
    pub step_id: String,
    pub required_capability: String, // Compatibility facade
    pub detail: Option<String>,      // Compatibility facade
    pub primitive: Option<PrimitiveSpec>,
    pub constraints: Vec<Constraint>,
    pub artifact_requirements: Vec<ArtifactRequirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionRule {
    pub step_id: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecSpec {
    pub version: u32,
    pub spec_id: String,
    pub steps: Vec<StepSpec>,
    pub transitions: Vec<TransitionRule>, // Deprecated, use dependencies
    pub dependencies: Vec<Dependency>,
    pub policies: Vec<Policy>,
    pub artifact_schemas: BTreeMap<String, String>,
}

impl ExecSpec {
    pub fn new(
        version: u32,
        steps: Vec<StepSpec>,
        transitions: Vec<TransitionRule>,
        dependencies: Vec<Dependency>,
        policies: Vec<Policy>,
        artifact_schemas: BTreeMap<String, String>,
    ) -> Self {
        let mut spec = Self {
            version,
            spec_id: String::new(),
            steps,
            transitions,
            dependencies,
            policies,
            artifact_schemas,
        };
        spec.spec_id = spec.calculate_hash();
        spec
    }

    pub fn calculate_hash(&self) -> String {
        // Stable serialization helper without spec_id
        #[derive(Serialize)]
        struct HashingSpec<'a> {
            version: u32,
            steps: &'a [StepSpec],
            transitions: &'a [TransitionRule],
            dependencies: &'a [Dependency],
            policies: &'a [Policy],
            artifact_schemas: &'a BTreeMap<String, String>,
        }

        let temp = HashingSpec {
            version: self.version,
            steps: &self.steps,
            transitions: &self.transitions,
            dependencies: &self.dependencies,
            policies: &self.policies,
            artifact_schemas: &self.artifact_schemas,
        };

        let bytes = serde_json::to_vec(&temp).unwrap_or_default();
        let hash = blake3::hash(&bytes);
        hash.to_hex()[..16].to_string()
    }

    /// Validate the structural integrity of this ExecSpec.
    /// Returns a list of all validation errors found.
    pub fn validate(&self) -> Vec<crate::kernel_error::KernelError> {
        use crate::kernel_error::{SpecificationError, ValidationError};
        use std::collections::HashSet;

        let mut errors = Vec::new();

        // Collect all step IDs
        let step_ids: HashSet<&str> = self.steps.iter().map(|s| s.step_id.as_str()).collect();

        // Validate hash consistency
        let calculated = self.calculate_hash();
        if !self.spec_id.is_empty() && self.spec_id != calculated {
            errors.push(
                ValidationError::HashMismatch {
                    spec_id: self.spec_id.clone(),
                    calculated,
                }
                .into(),
            );
        }

        // Validate dependencies reference existing steps
        for dep in &self.dependencies {
            if !step_ids.contains(dep.step_id.as_str()) {
                errors.push(
                    SpecificationError::MissingStep {
                        step_id: dep.step_id.clone(),
                    }
                    .into(),
                );
            }
            for parent_id in &dep.depends_on {
                if !step_ids.contains(parent_id.as_str()) {
                    errors.push(
                        SpecificationError::InvalidDependency {
                            step_id: dep.step_id.clone(),
                            missing_dep: parent_id.clone(),
                        }
                        .into(),
                    );
                }
            }
        }

        // Validate no duplicate step IDs
        if step_ids.len() != self.steps.len() {
            errors.push(
                SpecificationError::MalformedSpec {
                    detail: "duplicate step_id detected".to_string(),
                }
                .into(),
            );
        }

        errors
    }

    /// Validate hash consistency. Returns Ok if valid, Err with structured error otherwise.
    pub fn validate_hash(&self) -> Result<(), crate::kernel_error::KernelError> {
        let calculated = self.calculate_hash();
        if self.spec_id != calculated {
            return Err(crate::kernel_error::ValidationError::HashMismatch {
                spec_id: self.spec_id.clone(),
                calculated,
            }
            .into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_spec_roundtrip_json() {
        let spec = ExecSpec::new(
            1,
            vec![StepSpec {
                step_id: "00_analyze".to_string(),
                required_capability: "Planner".to_string(),
                detail: Some("details".to_string()),
                primitive: None,
                constraints: vec![Constraint {
                    target: "worker".to_string(),
                    key: "capability".to_string(),
                    value: "Planner".to_string(),
                }],
                artifact_requirements: vec![],
                inputs: vec![],
                outputs: vec![],
                metadata: serde_json::Value::Null,
            }],
            vec![],
            vec![Dependency {
                step_id: "01_execute".to_string(),
                depends_on: vec!["00_analyze".to_string()],
            }],
            vec![Policy {
                name: "timeout".to_string(),
                value: "60s".to_string(),
            }],
            BTreeMap::new(),
        );

        let json = serde_json::to_string(&spec).expect("failed to serialize spec to json");
        let restored: ExecSpec =
            serde_json::from_str(&json).expect("failed to deserialize spec from json");
        assert_eq!(spec, restored);
    }
}
