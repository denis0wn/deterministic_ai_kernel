use crate::providers::storage::StorageProvider;
use anyhow::Result;

// Worker facades take an explicit database path; no global routing state
// (audit finding M3).

pub fn claim_worker(db: &str, task_id: &str, worker_id: &str) -> Result<()> {
    crate::providers::storage_for(db).claim_worker(task_id, worker_id)
}

pub fn start_step(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    crate::providers::storage_for(db).start_step(task_id, worker_id, step_id)
}

pub fn heartbeat(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    crate::providers::storage_for(db).heartbeat(task_id, worker_id, step_id)
}

pub fn fail_step(
    db: &str,
    task_id: &str,
    worker_id: &str,
    step_id: &str,
    reason: &str,
) -> Result<()> {
    crate::providers::storage_for(db).fail_step(task_id, worker_id, step_id, reason)
}

pub fn complete_step(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    crate::providers::storage_for(db).complete_step(task_id, worker_id, step_id)
}

#[cfg(test)]
mod tests {
    // Regression: these tests exercise the PRODUCTION classification
    // functions (the previous module tested private local clones that could
    // silently diverge — audit finding "tests of production clones").
    use crate::providers::storage::{classify_failure_outcome, outcome_to_event_type};
    use crate::workflow::contract::StepOutcome;

    #[test]
    fn classify_failure_outcome_maps_retry_prefixes() {
        assert_eq!(
            classify_failure_outcome("retry: network blip"),
            StepOutcome::RetryableFailure
        );
        assert_eq!(
            classify_failure_outcome("timeout waiting for lock"),
            StepOutcome::RetryableFailure
        );
    }

    #[test]
    fn classify_failure_outcome_maps_blocked_prefixes() {
        assert_eq!(
            classify_failure_outcome("blocked: waiting on dependency"),
            StepOutcome::Blocked
        );
    }

    #[test]
    fn classify_failure_outcome_is_fail_safe_retryable_by_default() {
        // Audit finding C4: unknown/provider errors must NEVER become
        // terminal by default; terminal classification is opt-in via
        // an explicit `fatal:` prefix.
        assert_eq!(
            classify_failure_outcome("syntax error"),
            StepOutcome::RetryableFailure
        );
        assert_eq!(
            classify_failure_outcome("primitive_execution_error: mlx request failed"),
            StepOutcome::RetryableFailure
        );
        assert_eq!(
            classify_failure_outcome("fatal: unrecoverable corruption"),
            StepOutcome::TerminalFailure
        );
    }

    #[test]
    fn outcome_to_event_type_maps_success_and_blocked() {
        assert_eq!(
            outcome_to_event_type(StepOutcome::Success),
            "STEP_COMPLETED"
        );
        assert_eq!(outcome_to_event_type(StepOutcome::Blocked), "STEP_FAILED");
    }

    // ── R4 (HD-3): stall signature detection ────────────────────────────
    use crate::providers::storage::stall_elapsed_secs_from_reason;

    #[test]
    fn stall_signature_detected_with_elapsed_secs() {
        let reason = "primitive_execution_error: mlx request TIMED OUT after 120s \
                      waiting for model response (endpoint accepted the connection \
                      but produced no tokens; model/server hang suspected)";
        assert_eq!(stall_elapsed_secs_from_reason(reason), Some(120));
        let short = "mlx request TIMED OUT after 2s waiting for model response";
        assert_eq!(stall_elapsed_secs_from_reason(short), Some(2));
    }

    #[test]
    fn stall_signature_absent_for_ordinary_failures() {
        for reason in [
            "primitive_execution_error: mlx connection failed (endpoint down)",
            "fatal: real tests failed with exit code 1",
            "retry: network blip",
            // marker present but no number+unit — must NOT fire
            "mlx request TIMED OUT after sometime",
        ] {
            assert_eq!(
                stall_elapsed_secs_from_reason(reason),
                None,
                "false stall detection: {reason}"
            );
        }
    }

    // ── R6: hard-cap signature detection ────────────────────────────────
    use crate::providers::storage::hard_timeout_secs_from_reason;

    #[test]
    fn hard_timeout_signature_detected() {
        let reason = "primitive_execution_error: mlx request HARD_TIMEOUT_EXCEEDED \
                      after 300s despite active chunks (generation never terminated)";
        assert_eq!(hard_timeout_secs_from_reason(reason), Some(300));
        // idle signature must NOT trigger the hard-cap detector
        assert_eq!(
            hard_timeout_secs_from_reason("mlx stream TIMED OUT after 45s waiting for next chunk"),
            None
        );
    }
}
