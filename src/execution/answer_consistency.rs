use crate::execution::summary::{final_answer_matches_summary, DeterministicSummary};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnswerConsistencyReport {
    pub final_answer_present: bool,
    pub final_answer_empty: bool,
    pub deterministic_replay: bool,
    pub summary_contradiction: bool,
    pub warnings: Vec<String>,
}

pub fn validate(
    final_answer: Option<&str>,
    summary: &DeterministicSummary,
) -> AnswerConsistencyReport {
    let final_answer_present = final_answer.is_some();
    let final_answer_empty = final_answer
        .map(|text| text.trim().is_empty())
        .unwrap_or(true);

    let summary_contradiction = final_answer
        .filter(|text| !text.trim().is_empty())
        .map(|text| !final_answer_matches_summary(text, summary))
        .unwrap_or(false);

    let mut warnings = Vec::new();

    if !final_answer_present {
        warnings.push("final_answer_missing".to_string());
    } else if final_answer_empty {
        warnings.push("final_answer_empty".to_string());
    }

    if summary_contradiction {
        warnings.push("final_answer_contradicts_summary".to_string());
    }

    AnswerConsistencyReport {
        final_answer_present,
        final_answer_empty,
        deterministic_replay: summary.replay_valid,
        summary_contradiction,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::summary::DeterministicSummary;

    fn success_summary() -> DeterministicSummary {
        DeterministicSummary {
            task_id: "task-1".to_string(),
            status: "success/committed".to_string(),
            committed_steps: 1,
            failed_attempts: 0,
            retry_count: 0,
            replay_valid: true,
            committed_effects: 1,
            rejected_effects: 0,
        }
    }

    #[test]
    fn validate_accepts_consistent_final_answer() {
        let summary = success_summary();
        let report = validate(Some("Workflow completed successfully."), &summary);

        assert!(report.final_answer_present);
        assert!(!report.final_answer_empty);
        assert!(report.deterministic_replay);
        assert!(!report.summary_contradiction);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn validate_flags_empty_final_answer() {
        let summary = success_summary();
        let report = validate(Some("   "), &summary);

        assert!(report.final_answer_present);
        assert!(report.final_answer_empty);
        assert!(report.deterministic_replay);
        assert!(!report.summary_contradiction);
        assert_eq!(report.warnings, vec!["final_answer_empty"]);
    }

    #[test]
    fn validate_flags_summary_contradiction() {
        let summary = success_summary();
        let report = validate(Some("The workflow failed due to terminal error."), &summary);

        assert!(report.final_answer_present);
        assert!(!report.final_answer_empty);
        assert!(report.deterministic_replay);
        assert!(report.summary_contradiction);
        assert_eq!(report.warnings, vec!["final_answer_contradicts_summary"]);
    }

    #[test]
    fn validate_flags_missing_final_answer() {
        let summary = success_summary();
        let report = validate(None, &summary);

        assert!(!report.final_answer_present);
        assert!(report.final_answer_empty);
        assert!(report.deterministic_replay);
        assert!(!report.summary_contradiction);
        assert_eq!(report.warnings, vec!["final_answer_missing"]);
    }
}
