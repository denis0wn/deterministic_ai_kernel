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
}
