//! PR4: PipelineReport, PlanFingerprint, PlanDiff — Replay serialization
//!
//! Invariants:
//! - I6: PipelineReport serializes and is stored in Replay / Artifact Registry
//! - I7: equal (Task, Seed, Manifest) → equal fingerprint

use crate::workflow::planner_types::{PlannerManifest, PlannerStep, StablePlannerId};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// PlanFingerprint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanFingerprint(pub String);

impl PlanFingerprint {
    pub fn compute(steps: &[PlannerStep], manifest: &PlannerManifest) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(manifest.pipeline_version.as_bytes());
        hasher.update(b"\x00");
        hasher.update(manifest.planner_version.as_bytes());
        hasher.update(b"\x00");
        for ps in steps {
            hasher.update(ps.id.0.as_bytes());
            hasher.update(b"\x01");
        }
        let hash = hasher.finalize();
        Self(format!("{:.16}", hash.to_hex()))
    }
}

impl std::fmt::Display for PlanFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// StepDelta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "delta")]
pub enum StepDelta {
    Added {
        id: StablePlannerId,
    },
    Removed {
        id: StablePlannerId,
    },
    Moved {
        id: StablePlannerId,
        from: usize,
        to: usize,
    },
    Changed {
        id: StablePlannerId,
        field: String,
    },
}

// ---------------------------------------------------------------------------
// PlanDiff
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanDiff {
    pub deltas: Vec<StepDelta>,
}

impl PlanDiff {
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty()
    }

    pub fn compute(before: &[PlannerStep], after: &[PlannerStep]) -> Self {
        let before_ids: Vec<&StablePlannerId> = before.iter().map(|ps| &ps.id).collect();
        let after_ids: Vec<&StablePlannerId> = after.iter().map(|ps| &ps.id).collect();
        let mut deltas = Vec::new();

        for id in &before_ids {
            if !after_ids.contains(id) {
                deltas.push(StepDelta::Removed { id: (*id).clone() });
            }
        }
        for id in &after_ids {
            if !before_ids.contains(id) {
                deltas.push(StepDelta::Added { id: (*id).clone() });
            }
        }
        for (ai, id) in after_ids.iter().enumerate() {
            if let Some(bi) = before_ids.iter().position(|bid| bid == id) {
                if bi != ai {
                    deltas.push(StepDelta::Moved {
                        id: (*id).clone(),
                        from: bi,
                        to: ai,
                    });
                }
            }
        }
        for bps in before.iter() {
            if let Some(aps) = after.iter().find(|ps| ps.id == bps.id) {
                if aps.step.detail != bps.step.detail {
                    deltas.push(StepDelta::Changed {
                        id: bps.id.clone(),
                        field: "detail".into(),
                    });
                }
            }
        }

        Self { deltas }
    }
}

// ---------------------------------------------------------------------------
// PipelineReport
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineReport {
    pub task_id: String,
    pub seed: u64,
    pub manifest: PlannerManifest,
    pub fingerprint: PlanFingerprint,
    pub steps: Vec<PlannerStep>,
    pub issue_count: usize,
    pub recovered: bool,
}

impl PipelineReport {
    pub fn new(
        task_id: String,
        seed: u64,
        manifest: PlannerManifest,
        steps: Vec<PlannerStep>,
        issue_count: usize,
        recovered: bool,
    ) -> Self {
        let fingerprint = PlanFingerprint::compute(&steps, &manifest);
        Self {
            task_id,
            seed,
            manifest,
            fingerprint,
            steps,
            issue_count,
            recovered,
        }
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::contract::{Step, StepKind};
    use crate::workflow::planner_types::{PlannerManifest, PlannerStep};

    fn manifest() -> PlannerManifest {
        PlannerManifest::v1()
    }

    fn make_step(kind: StepKind, seed: u64) -> PlannerStep {
        PlannerStep::from_step(Step { kind, detail: None }, &manifest(), seed)
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let steps = vec![
            make_step(StepKind::AnalyzeTask, 0),
            make_step(StepKind::RunTests, 0),
        ];
        let m = manifest();
        assert_eq!(
            PlanFingerprint::compute(&steps, &m),
            PlanFingerprint::compute(&steps, &m)
        );
    }

    #[test]
    fn fingerprint_differs_by_order() {
        let m = manifest();
        let s1 = vec![
            make_step(StepKind::AnalyzeTask, 0),
            make_step(StepKind::RunTests, 0),
        ];
        let s2 = vec![
            make_step(StepKind::RunTests, 0),
            make_step(StepKind::AnalyzeTask, 0),
        ];
        assert_ne!(
            PlanFingerprint::compute(&s1, &m),
            PlanFingerprint::compute(&s2, &m)
        );
    }

    #[test]
    fn plan_diff_detects_added() {
        let before = vec![make_step(StepKind::AnalyzeTask, 0)];
        let after = vec![
            make_step(StepKind::AnalyzeTask, 0),
            make_step(StepKind::RunTests, 0),
        ];
        let diff = PlanDiff::compute(&before, &after);
        assert_eq!(diff.deltas.len(), 1);
        assert!(matches!(&diff.deltas[0], StepDelta::Added { .. }));
    }

    #[test]
    fn plan_diff_detects_removed() {
        let before = vec![
            make_step(StepKind::AnalyzeTask, 0),
            make_step(StepKind::RunTests, 0),
        ];
        let after = vec![make_step(StepKind::AnalyzeTask, 0)];
        let diff = PlanDiff::compute(&before, &after);
        assert_eq!(diff.deltas.len(), 1);
        assert!(matches!(&diff.deltas[0], StepDelta::Removed { .. }));
    }

    #[test]
    fn plan_diff_identical_is_empty() {
        let steps = vec![make_step(StepKind::AnalyzeTask, 0)];
        assert!(PlanDiff::compute(&steps, &steps).is_empty());
    }

    #[test]
    fn pipeline_report_roundtrips_json() {
        let steps = vec![make_step(StepKind::AnalyzeTask, 42)];
        let report = PipelineReport::new("task-1".into(), 42, manifest(), steps, 0, false);
        let json = report.to_json().unwrap();
        let restored = PipelineReport::from_json(&json).unwrap();
        assert_eq!(report, restored);
    }

    #[test]
    fn same_inputs_yield_same_fingerprint() {
        let m = manifest();
        let sa = vec![
            make_step(StepKind::AnalyzeTask, 7),
            make_step(StepKind::RunTests, 7),
        ];
        let sb = vec![
            make_step(StepKind::AnalyzeTask, 7),
            make_step(StepKind::RunTests, 7),
        ];
        assert_eq!(
            PlanFingerprint::compute(&sa, &m),
            PlanFingerprint::compute(&sb, &m)
        );
    }
}
