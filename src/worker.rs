use anyhow::Result;

pub fn claim_worker(db: &str, task_id: &str, worker_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().claim_worker(task_id, worker_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

pub fn start_step(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().start_step(task_id, worker_id, step_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

pub fn heartbeat(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().heartbeat(task_id, worker_id, step_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

pub fn fail_step(
    db: &str,
    task_id: &str,
    worker_id: &str,
    step_id: &str,
    reason: &str,
) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().fail_step(task_id, worker_id, step_id, reason);
    crate::providers::get_storage().set_override_path(None);
    res
}

pub fn complete_step(db: &str, task_id: &str, worker_id: &str, step_id: &str) -> Result<()> {
    crate::providers::get_storage().set_override_path(Some(db.to_string()));
    let res = crate::providers::get_storage().complete_step(task_id, worker_id, step_id);
    crate::providers::get_storage().set_override_path(None);
    res
}

#[cfg(test)]
mod tests {
    use crate::workflow::contract::StepOutcome;

    fn classify_failure_outcome(reason: &str) -> StepOutcome {
        let lower = reason.trim().to_ascii_lowercase();

        if lower.starts_with("retry:")
            || lower.starts_with("transient:")
            || lower.starts_with("timeout")
        {
            return StepOutcome::RetryableFailure;
        }

        if lower.starts_with("blocked:")
            || lower.starts_with("waiting_on:")
            || lower.starts_with("dependency:")
        {
            return StepOutcome::Blocked;
        }

        StepOutcome::TerminalFailure
    }

    fn outcome_to_event_type(outcome: StepOutcome) -> &'static str {
        match outcome {
            StepOutcome::Success => "STEP_COMPLETED",
            StepOutcome::RetryableFailure | StepOutcome::TerminalFailure | StepOutcome::Blocked => {
                "STEP_FAILED"
            }
        }
    }

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
    fn classify_failure_outcome_defaults_to_terminal_failure() {
        assert_eq!(
            classify_failure_outcome("syntax error"),
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
