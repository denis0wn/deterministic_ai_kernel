#![allow(dead_code, unused)]
//! PR2: PlannerCritic::analyze + PlannerRecover::apply
//!
//! Invariants:
//! - I3: analyze() is idempotent — Analyze(Analyze(plan)) == Analyze(plan)
//! - I3: analyze() never mutates — only observes and reports issues
//! - Recovery is a separate pass (PlannerRecover::apply)

use crate::workflow::contract::{Step, StepKind};
use crate::workflow::planner_types::{PlannerManifest, PlannerStep, StepProvenance};

// ---------------------------------------------------------------------------
// CriticIssue
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriticIssue {
    /// Plan contains no steps at all.
    EmptyPlan,
    /// A required step kind is missing from the plan.
    MissingStep { kind: StepKind },
    /// The same step kind appears more than once.
    DuplicateStep { kind: StepKind },
}

// ---------------------------------------------------------------------------
// CriticReport
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticReport {
    pub issues: Vec<CriticIssue>,
}

impl CriticReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

// ---------------------------------------------------------------------------
// PlannerCritic — analyze only, never mutates (I3)
// ---------------------------------------------------------------------------

pub struct PlannerCritic;

impl PlannerCritic {
    /// Analyze a plan and return a report of issues.
    ///
    /// Idempotent: calling analyze on an already-analyzed plan
    /// produces the same report. Never modifies the input.
    pub fn analyze(steps: &[PlannerStep]) -> CriticReport {
        let mut issues = Vec::new();

        if steps.is_empty() {
            issues.push(CriticIssue::EmptyPlan);
            return CriticReport { issues };
        }

        // Duplicate detection
        let mut seen: Vec<&StepKind> = Vec::new();
        for ps in steps {
            let kind = &ps.step.kind;
            if seen.contains(&kind) {
                if !issues.contains(&CriticIssue::DuplicateStep { kind: kind.clone() }) {
                    issues.push(CriticIssue::DuplicateStep { kind: kind.clone() });
                }
            } else {
                seen.push(kind);
            }
        }

        CriticReport { issues }
    }
}

// ---------------------------------------------------------------------------
// RecoveryAction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Insert a step with the given kind.
    InsertStep { kind: StepKind },
    /// Remove duplicate steps, keeping the first occurrence.
    DeduplicateSteps,
}

// ---------------------------------------------------------------------------
// PlannerRecover — apply recovery actions (I3: separate from analyze)
// ---------------------------------------------------------------------------

pub struct PlannerRecover;

impl PlannerRecover {
    /// Derive recovery actions from a CriticReport.
    pub fn plan_recovery(report: &CriticReport) -> Vec<RecoveryAction> {
        let mut actions = Vec::new();
        for issue in &report.issues {
            match issue {
                CriticIssue::EmptyPlan => {
                    actions.push(RecoveryAction::InsertStep {
                        kind: StepKind::AnalyzeTask,
                    });
                }
                CriticIssue::MissingStep { kind } => {
                    actions.push(RecoveryAction::InsertStep { kind: kind.clone() });
                }
                CriticIssue::DuplicateStep { .. } => {
                    if !actions.contains(&RecoveryAction::DeduplicateSteps) {
                        actions.push(RecoveryAction::DeduplicateSteps);
                    }
                }
            }
        }
        actions
    }

    /// Apply recovery actions to a plan, returning a new plan.
    /// Never modifies the original — returns a new Vec.
    pub fn apply(
        steps: Vec<PlannerStep>,
        actions: &[RecoveryAction],
        manifest: &PlannerManifest,
        seed: u64,
    ) -> Vec<PlannerStep> {
        let mut result = steps;

        for action in actions {
            match action {
                RecoveryAction::InsertStep { kind } => {
                    let step = Step {
                        kind: kind.clone(),
                        detail: None,
                    };
                    let ps = PlannerStep::from_step(step, manifest, seed).with_provenance(
                        StepProvenance::CriticRecovery {
                            rule: format!("insert_{:?}", kind),
                        },
                    );
                    result.push(ps);
                }
                RecoveryAction::DeduplicateSteps => {
                    let mut seen: Vec<StepKind> = Vec::new();
                    result.retain(|ps| {
                        if seen.contains(&ps.step.kind) {
                            false
                        } else {
                            seen.push(ps.step.kind.clone());
                            true
                        }
                    });
                }
            }
        }

        result
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

    fn make_step(kind: StepKind) -> PlannerStep {
        PlannerStep::from_step(Step { kind, detail: None }, &manifest(), 0)
    }

    // I3: idempotency
    #[test]
    fn analyze_is_idempotent() {
        let steps = vec![
            make_step(StepKind::AnalyzeTask),
            make_step(StepKind::RunTests),
            make_step(StepKind::RunTests), // duplicate
        ];
        let r1 = PlannerCritic::analyze(&steps);
        let r2 = PlannerCritic::analyze(&steps);
        assert_eq!(r1, r2);
    }

    #[test]
    fn analyze_detects_empty_plan() {
        let report = PlannerCritic::analyze(&[]);
        assert!(report.issues.contains(&CriticIssue::EmptyPlan));
    }

    #[test]
    fn analyze_detects_duplicates() {
        let steps = vec![make_step(StepKind::RunTests), make_step(StepKind::RunTests)];
        let report = PlannerCritic::analyze(&steps);
        assert!(report.issues.contains(&CriticIssue::DuplicateStep {
            kind: StepKind::RunTests
        }));
    }

    #[test]
    fn analyze_clean_plan_has_no_issues() {
        let steps = vec![
            make_step(StepKind::AnalyzeTask),
            make_step(StepKind::RunTests),
        ];
        let report = PlannerCritic::analyze(&steps);
        assert!(report.is_clean());
    }

    // I3: analyze never mutates — input unchanged after call
    #[test]
    fn analyze_does_not_mutate_input() {
        let steps = vec![make_step(StepKind::AnalyzeTask)];
        let before = steps.clone();
        let _ = PlannerCritic::analyze(&steps);
        assert_eq!(steps, before);
    }

    #[test]
    fn recover_empty_plan_inserts_analyze_task() {
        let report = CriticReport {
            issues: vec![CriticIssue::EmptyPlan],
        };
        let actions = PlannerRecover::plan_recovery(&report);
        assert!(actions.contains(&RecoveryAction::InsertStep {
            kind: StepKind::AnalyzeTask
        }));
        let result = PlannerRecover::apply(vec![], &actions, &manifest(), 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].step.kind, StepKind::AnalyzeTask);
    }

    #[test]
    fn recover_deduplicates_steps() {
        let steps = vec![make_step(StepKind::RunTests), make_step(StepKind::RunTests)];
        let report = PlannerCritic::analyze(&steps);
        let actions = PlannerRecover::plan_recovery(&report);
        let result = PlannerRecover::apply(steps, &actions, &manifest(), 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].step.kind, StepKind::RunTests);
    }

    #[test]
    fn recovered_steps_have_critic_provenance() {
        let report = CriticReport {
            issues: vec![CriticIssue::EmptyPlan],
        };
        let actions = PlannerRecover::plan_recovery(&report);
        let result = PlannerRecover::apply(vec![], &actions, &manifest(), 0);
        assert!(matches!(
            result[0].provenance,
            StepProvenance::CriticRecovery { .. }
        ));
    }

    // Property: apply(apply(steps)) == apply(steps) when report is clean after first pass
    #[test]
    fn recovery_is_stable_on_second_pass() {
        let steps = vec![make_step(StepKind::RunTests), make_step(StepKind::RunTests)];
        let r1 = PlannerCritic::analyze(&steps);
        let a1 = PlannerRecover::plan_recovery(&r1);
        let after_first = PlannerRecover::apply(steps, &a1, &manifest(), 0);

        let r2 = PlannerCritic::analyze(&after_first);
        let a2 = PlannerRecover::plan_recovery(&r2);
        let after_second = PlannerRecover::apply(after_first.clone(), &a2, &manifest(), 0);

        assert_eq!(after_first, after_second);
    }
}
