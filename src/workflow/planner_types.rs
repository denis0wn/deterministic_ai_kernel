//! PR1: StablePlannerId, StepProvenance, PlannerStep, PlannerManifest

use crate::workflow::contract::Step;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannerManifest {
    pub planner_version: String,
    pub pipeline_version: String,
    pub semantic_version: String,
    pub schema_version: String,
    pub critic_version: String,
    pub artifact_version: String,
}

impl PlannerManifest {
    pub fn v1() -> Self {
        Self {
            planner_version: "1.0.0".into(),
            pipeline_version: "1.0.0".into(),
            semantic_version: "1.0.0".into(),
            schema_version: "1.0.0".into(),
            critic_version: "1.0.0".into(),
            artifact_version: "1.0.0".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StablePlannerId(pub String);

impl StablePlannerId {
    pub fn compute(
        planner_version: &str,
        step_kind: &str,
        normalized_description: &str,
        seed: u64,
    ) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(planner_version.as_bytes());
        hasher.update(b"\x00");
        hasher.update(step_kind.as_bytes());
        hasher.update(b"\x00");
        hasher.update(normalized_description.as_bytes());
        hasher.update(b"\x00");
        hasher.update(&seed.to_le_bytes());
        let hash = hasher.finalize();
        Self(format!("{:.16}", hash.to_hex()))
    }
}

impl std::fmt::Display for StablePlannerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "source")]
pub enum StepProvenance {
    #[default]
    Unknown,
    TaskParser {
        task_id: String,
    },
    CriticRecovery {
        rule: String,
    },
    Replay {
        capsule_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannerStep {
    pub step: Step,
    pub id: StablePlannerId,
    pub provenance: StepProvenance,
}

use crate::workflow::contract::StepKind;

impl AsRef<str> for StepKind {
    fn as_ref(&self) -> &str {
        match self {
            StepKind::TightenPlannerPrompt => "tighten planner prompt",
            StepKind::NormalizePlannerOutput => "normalize planner output",
            StepKind::AddLlmFallbackHandling => "add llm fallback handling",
            StepKind::AddPlannerTestCoverage => "add planner test coverage",
            StepKind::ValidatePlannerOutput => "validate planner output",
            StepKind::AnalyzeTask => "analyze task",
            StepKind::PlanExecution => "plan execution",
            StepKind::ExecuteChanges => "execute changes",
            StepKind::ReadRepository => "read repository",
            StepKind::LocateBug => "locate bug",
            StepKind::PatchCode => "patch code",
            StepKind::RunTests => "run tests",
            StepKind::ValidatePatch => "validate patch",
        }
    }
}

impl PlannerStep {
    pub fn from_step(step: Step, manifest: &PlannerManifest, seed: u64) -> Self {
        let step_kind = format!("{:?}", step.kind);
        let normalized_description = step.detail.as_deref().unwrap_or_else(|| step.kind.as_ref());
        let id = StablePlannerId::compute(
            &manifest.planner_version,
            &step_kind,
            normalized_description,
            seed,
        );
        Self {
            step,
            id,
            provenance: StepProvenance::Unknown,
        }
    }

    pub fn with_provenance(mut self, provenance: StepProvenance) -> Self {
        self.provenance = provenance;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::contract::{Step, StepKind};

    fn manifest() -> PlannerManifest {
        PlannerManifest::v1()
    }

    #[test]
    fn stable_planner_id_is_deterministic() {
        let id1 = StablePlannerId::compute("1.0.0", "AnalyzeTask", "analyze task", 42);
        let id2 = StablePlannerId::compute("1.0.0", "AnalyzeTask", "analyze task", 42);
        assert_eq!(id1, id2);
    }

    #[test]
    fn stable_planner_id_differs_by_seed() {
        let id1 = StablePlannerId::compute("1.0.0", "AnalyzeTask", "analyze task", 1);
        let id2 = StablePlannerId::compute("1.0.0", "AnalyzeTask", "analyze task", 2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn stable_planner_id_differs_by_kind() {
        let id1 = StablePlannerId::compute("1.0.0", "AnalyzeTask", "analyze task", 0);
        let id2 = StablePlannerId::compute("1.0.0", "RunTests", "run tests", 0);
        assert_ne!(id1, id2);
    }

    #[test]
    fn planner_step_default_provenance_is_unknown() {
        let step = Step {
            kind: StepKind::AnalyzeTask,
            detail: None,
        };
        let ps = PlannerStep::from_step(step, &manifest(), 0);
        assert_eq!(ps.provenance, StepProvenance::Unknown);
    }

    #[test]
    fn planner_step_id_stable_across_positions() {
        let s1 = Step {
            kind: StepKind::RunTests,
            detail: None,
        };
        let s2 = Step {
            kind: StepKind::RunTests,
            detail: None,
        };
        let ps1 = PlannerStep::from_step(s1, &manifest(), 7);
        let ps2 = PlannerStep::from_step(s2, &manifest(), 7);
        assert_eq!(ps1.id, ps2.id);
    }

    #[test]
    fn manifest_v1_has_all_fields() {
        let m = PlannerManifest::v1();
        assert!(!m.planner_version.is_empty());
        assert!(!m.pipeline_version.is_empty());
        assert!(!m.semantic_version.is_empty());
        assert!(!m.schema_version.is_empty());
        assert!(!m.critic_version.is_empty());
        assert!(!m.artifact_version.is_empty());
    }

    #[test]
    fn step_provenance_serializes_roundtrip() {
        let p = StepProvenance::CriticRecovery {
            rule: "missing_test".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: StepProvenance = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }
}
