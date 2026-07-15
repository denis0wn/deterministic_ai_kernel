use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrimitiveId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveKind {
    Compute,
    Read,
    Write,
    Route,
    Wait,
    Signal,
    Spawn,
    Complete,
    Fail,
    SolveConstraint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrimitiveSpec {
    pub id: PrimitiveId,
    pub kind: PrimitiveKind,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactSpec {
    pub artifact_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrimitiveResult {
    pub id: PrimitiveId,
    pub status: String,
    pub output: serde_json::Value,
    pub artifacts: Vec<ArtifactSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrimitiveTransition {
    pub from: PrimitiveId,
    pub to: PrimitiveId,
    pub condition: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::contract::StepKind;

    #[test]
    fn primitives_roundtrip_json() {
        let spec = PrimitiveSpec {
            id: PrimitiveId("prim-1".to_string()),
            kind: PrimitiveKind::Compute,
            payload: serde_json::json!({ "arg": 42 }),
        };
        let json =
            serde_json::to_string(&spec).expect("failed to serialize primitive spec to json");
        let restored: PrimitiveSpec =
            serde_json::from_str(&json).expect("failed to deserialize primitive spec from json");
        assert_eq!(spec, restored);
    }

    #[test]
    fn step_kind_to_primitive_kind_mapping() {
        assert_eq!(
            StepKind::ReadRepository.to_primitive_kind(),
            PrimitiveKind::Read
        );
        assert_eq!(
            StepKind::PatchCode.to_primitive_kind(),
            PrimitiveKind::Write
        );
        assert_eq!(
            StepKind::AnalyzeTask.to_primitive_kind(),
            PrimitiveKind::Compute
        );
        assert_eq!(
            StepKind::ValidatePatch.to_primitive_kind(),
            PrimitiveKind::Route
        );
    }
}
