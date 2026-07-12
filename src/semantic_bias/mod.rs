use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiasVersion {
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticBiasRule {
    pub id: String,
    pub priority: u8,
    pub condition: String,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BiasConfiguration {
    pub version: BiasVersion,
    pub id: String,
    pub rules: Vec<SemanticBiasRule>,
}

impl BiasVersion {
    pub fn current() -> Self {
        Self::V1
    }
}

impl SemanticBiasRule {
    pub fn new(
        id: impl Into<String>,
        priority: u8,
        condition: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            priority,
            condition: condition.into(),
            action: action.into(),
        }
    }
}

impl BiasConfiguration {
    pub fn new(id: impl Into<String>, rules: Vec<SemanticBiasRule>) -> Self {
        Self {
            version: BiasVersion::current(),
            id: id.into(),
            rules,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bias_configuration_roundtrip_json() {
        let cfg = BiasConfiguration::new(
            "test-bias",
            vec![SemanticBiasRule::new(
                "rule-1",
                10,
                "state==pending",
                "prefer_low_latency",
            )],
        );
        let json = serde_json::to_string(&cfg).expect("configuration serialization failed");
        let restored: BiasConfiguration =
            serde_json::from_str(&json).expect("configuration deserialization failed");
        assert_eq!(cfg, restored);
    }

    #[test]
    fn bias_version_is_v1_by_default() {
        assert_eq!(BiasVersion::current(), BiasVersion::V1);
    }
}
