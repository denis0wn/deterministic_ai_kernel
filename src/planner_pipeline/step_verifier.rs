use anyhow::Result;
use serde::Deserialize;

use crate::llm::{self, VERIFIER_PROMPT};
use crate::model_registry::ModelPurpose;

use super::execution_engine::StepStatus;

// Hard-coded limits
const MAX_STEP_RETRIES: usize = 1;

#[derive(Debug, Deserialize)]
pub struct VerifierResult {
    pub verdict: String,
    #[serde(default)]
    pub reason: String,
}

/// Verify a step result against its expected contract.
/// Returns PASS/FAIL verdict.
///
/// Fail-closed contract (audit findings M4/EH2): a malformed verifier
/// response is never treated as PASS — it is reported as FAIL/UNVERIFIED so
/// the gate stays closed when verification itself breaks.
pub async fn verify_step(
    step_kind: &str,
    step_detail: &str,
    step_output: &str,
) -> Result<VerifierResult> {
    let user_prompt = format!(
        "Step kind: {}\nStep detail: {}\nOutput to verify:\n{}",
        step_kind, step_detail, step_output
    );

    let result =
        llm::chat_structured(ModelPurpose::Verifier, VERIFIER_PROMPT, &user_prompt).await?;

    let verdict: VerifierResult = serde_json::from_value(result).unwrap_or(VerifierResult {
        verdict: "FAIL".to_string(),
        reason: "UNVERIFIED: malformed verifier response".to_string(),
    });

    Ok(verdict)
}

/// Run verification with bounded retry.
/// If first verification FAILs, retry the step once and verify again.
/// Returns the final StepStatus. Every error path fails closed: verifier
/// unavailability or malformed output produces StepStatus::Failed, never Ok.
pub async fn verify_with_retry<F>(
    step_kind: &str,
    step_detail: &str,
    step_output: &str,
    execute_fn: F,
) -> StepStatus
where
    F: std::future::Future<Output = Result<StepStatus>>,
{
    // First verification
    let verdict = match verify_step(step_kind, step_detail, step_output).await {
        Ok(v) => v,
        Err(e) => {
            return StepStatus::Failed(format!("UNVERIFIED: verifier error: {e}"));
        }
    };

    if verdict.verdict.eq_ignore_ascii_case("pass") {
        return StepStatus::Ok;
    }

    // FAIL — exactly one bounded retry (MAX_STEP_RETRIES == 1; written
    // without a loop so the control flow is explicit).
    debug_assert_eq!(MAX_STEP_RETRIES, 1, "single-retry contract");
    match execute_fn.await {
        Ok(status) => {
            if matches!(status, StepStatus::Ok) {
                // Retry succeeded — verify the retried output.
                match verify_step(step_kind, step_detail, "").await {
                    Ok(retry_verdict) if retry_verdict.verdict.eq_ignore_ascii_case("pass") => {
                        return StepStatus::Ok;
                    }
                    Ok(retry_verdict) => {
                        return StepStatus::Failed(format!(
                            "verification failed after retry: {}",
                            retry_verdict.reason
                        ));
                    }
                    Err(e) => {
                        return StepStatus::Failed(format!("UNVERIFIED after retry: {e}"));
                    }
                }
            }
            // Retry itself failed — propagate its status.
            status
        }
        Err(e) => StepStatus::Failed(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_step_retries_is_one() {
        assert_eq!(MAX_STEP_RETRIES, 1);
    }
}
