use deterministic_ai_kernel::execution::runtime::ExecutionReceipt;
use deterministic_ai_kernel::execution::summary::{
    calculate_receipt_hash, final_answer_matches_summary, format_deterministic_fallback,
    render_final_answer_from_summary, DeterministicSummary,
};
use tempfile::NamedTempFile;

fn setup_test_db() -> (NamedTempFile, String) {
    let file = NamedTempFile::new().unwrap();
    let db_path = file.path().to_str().unwrap().to_string();

    // Set override and run a simple read to trigger default schema boot creation
    deterministic_ai_kernel::providers::get_storage().set_override_path(Some(db_path.clone()));
    let _ = deterministic_ai_kernel::providers::get_storage().get_effect_counts("init-task");
    deterministic_ai_kernel::providers::get_storage().set_override_path(None);

    (file, db_path)
}

#[tokio::test]
async fn test_final_answer_matches_summary_contradictions() {
    let summary_success = DeterministicSummary {
        task_id: "task-1".to_string(),
        status: "success/committed".to_string(),
        committed_steps: 3,
        failed_attempts: 1,
        retry_count: 1,
        replay_valid: true,
        cache_hits: 1,
        committed_effects: 2,
        rejected_effects: 0,
    };

    // 1. Success status, but text says failed/failure
    assert!(!final_answer_matches_summary(
        "Workflow failed on step 2",
        &summary_success
    ));
    assert!(!final_answer_matches_summary(
        "Step execution encountered a fatal failure",
        &summary_success
    ));
    assert!(!final_answer_matches_summary(
        "Task was unsuccessful",
        &summary_success
    ));
    assert!(!final_answer_matches_summary(
        "Execution stopped due to a terminal error",
        &summary_success
    ));

    // 2. Success status, text is valid
    assert!(final_answer_matches_summary(
        "Workflow completed successfully with 3 steps.",
        &summary_success
    ));

    // 3. Replay is valid, but text claims corruption/invalid replay
    let summary_replay_valid = DeterministicSummary {
        replay_valid: true,
        ..summary_success.clone()
    };
    assert!(!final_answer_matches_summary(
        "Replay is invalid",
        &summary_replay_valid
    ));
    assert!(!final_answer_matches_summary(
        "Replay verification failed due to corruption",
        &summary_replay_valid
    ));

    // 4. Replay is valid, text is valid
    assert!(final_answer_matches_summary(
        "Replay validation passed successfully",
        &summary_replay_valid
    ));
}

#[tokio::test]
async fn test_fallback_path_on_validation_failure() {
    // Set mock backend
    std::env::set_var("DAK_LM_BACKEND", "mock");

    // Force contradiction using the special task_id trigger
    let summary = DeterministicSummary {
        task_id: "FORCE_CONTRADICTION".to_string(),
        status: "success/committed".to_string(),
        committed_steps: 3,
        failed_attempts: 1,
        retry_count: 1,
        replay_valid: true,
        cache_hits: 0,
        committed_effects: 1,
        rejected_effects: 0,
    };

    let result = render_final_answer_from_summary(&summary).await;
    assert!(result.is_err());
    let err_msg = result.err().unwrap().to_string();
    assert_eq!(err_msg, "FINAL_ANSWER_CONTRADICTS_RECEIPT");

    // Fallback path text matches expectations
    let fallback = format_deterministic_fallback(&summary);
    assert_eq!(
        fallback,
        "Workflow completed successfully. Completed steps: 3. Retries: 1. Replay: valid."
    );
}

#[tokio::test]
async fn test_transient_fail_retry_committed_no_failure_wording() {
    std::env::set_var("DAK_LM_BACKEND", "mock");

    let summary = DeterministicSummary {
        task_id: "transient-success-retry".to_string(),
        status: "success/committed".to_string(),
        committed_steps: 3,
        failed_attempts: 1,
        retry_count: 1,
        replay_valid: true,
        cache_hits: 0,
        committed_effects: 1,
        rejected_effects: 0,
    };

    let result = render_final_answer_from_summary(&summary).await.unwrap();
    assert!(final_answer_matches_summary(&result, &summary));
    assert!(!result.to_lowercase().contains("failed"));
    assert!(!result.to_lowercase().contains("failure"));
}

#[tokio::test]
async fn test_replay_invalid_reflected_in_answer() {
    std::env::set_var("DAK_LM_BACKEND", "mock");

    let summary = DeterministicSummary {
        task_id: "invalid-replay-task".to_string(),
        status: "failed".to_string(),
        committed_steps: 2,
        failed_attempts: 1,
        retry_count: 1,
        replay_valid: false,
        cache_hits: 0,
        committed_effects: 1,
        rejected_effects: 0,
    };

    let result = render_final_answer_from_summary(&summary).await.unwrap();
    assert!(final_answer_matches_summary(&result, &summary));
    assert!(result.to_lowercase().contains("invalid"));
}

#[tokio::test]
async fn test_canonical_receipt_hash_stability() {
    let receipt1 = ExecutionReceipt {
        task_id: "t1".to_string(),
        status: "completed".to_string(),
        completed_steps: 3,
        failed_attempts: 0,
        retry_count: 0,
        recovery_events: 0,
        cache_hits: 0,
        tool_calls: 3,
        llm_calls: 1,
        artifacts: 1,
        replay_validation: true,
        wall_clock_ms: 1234,
        planner_ms: 200,
        compiler_ms: 50,
        scheduler_ms: 100,
        execution_ms: 800,
        recovery_ms: 0,
        replay_ms: 84,
    };

    let receipt2 = ExecutionReceipt {
        // exactly identical properties
        ..receipt1.clone()
    };

    let hash1 = calculate_receipt_hash(&receipt1);
    let hash2 = calculate_receipt_hash(&receipt2);
    assert_eq!(hash1, hash2);
}

#[tokio::test]
async fn test_db_receipt_summary_construction_flow() {
    let (_tmp, db_path) = setup_test_db();
    deterministic_ai_kernel::providers::get_storage().set_override_path(Some(db_path.clone()));

    // 1. Insert step statuses
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    conn.execute(
        "INSERT INTO step_status (task_id, step_id, status) VALUES ('task-x', 'step_1', 'committed')",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO step_status (task_id, step_id, status) VALUES ('task-x', 'step_2', 'committed')",
        [],
    ).unwrap();

    // 2. Insert committed effects
    conn.execute(
        "INSERT INTO effect_ledger (effect_id, task_id, step_id, reservation_generation, state) VALUES ('eff-1', 'task-x', 'step_2', 1, 'committed')",
        [],
    ).unwrap();

    // 3. Verify get_effect_counts
    let (comm, rej) = deterministic_ai_kernel::providers::get_storage()
        .get_effect_counts("task-x")
        .unwrap();
    assert_eq!(comm, 1);
    assert_eq!(rej, 0);

    // 4. Verify DeterministicSummary build
    let receipt = ExecutionReceipt {
        task_id: "task-x".to_string(),
        status: "completed".to_string(),
        completed_steps: 2,
        failed_attempts: 0,
        retry_count: 0,
        recovery_events: 0,
        cache_hits: 0,
        tool_calls: 2,
        llm_calls: 1,
        artifacts: 1,
        replay_validation: true,
        wall_clock_ms: 1000,
        planner_ms: 200,
        compiler_ms: 100,
        scheduler_ms: 100,
        execution_ms: 600,
        recovery_ms: 0,
        replay_ms: 50,
    };

    let summary = DeterministicSummary::build(&receipt).unwrap();
    assert_eq!(summary.status, "success/committed");
    assert_eq!(summary.committed_steps, 2);
    assert_eq!(summary.committed_effects, 1);
    assert_eq!(summary.rejected_effects, 0);

    // Reset override
    deterministic_ai_kernel::providers::get_storage().set_override_path(None);
}

#[tokio::test]
async fn test_regression_second_step_failed_once_recovered_success() {
    std::env::set_var("DAK_LM_BACKEND", "mock");

    // "второй шаг упал один раз и сразу встал"
    // All steps are committed, status is success/committed, but there was 1 failed attempt and 1 retry.
    let summary = DeterministicSummary {
        task_id: "second-step-transient-fail".to_string(),
        status: "success/committed".to_string(),
        committed_steps: 3,
        failed_attempts: 1,
        retry_count: 1,
        replay_valid: true,
        cache_hits: 0,
        committed_effects: 1,
        rejected_effects: 0,
    };

    let human_summary = render_final_answer_from_summary(&summary).await.unwrap();

    // Asserts
    assert!(final_answer_matches_summary(&human_summary, &summary));
    assert!(!human_summary.to_lowercase().contains("failed"));
    assert!(!human_summary.to_lowercase().contains("failure"));

    // The summary must explicitly report success (or "completed successfully" in mock mode)
    assert!(human_summary.contains("completed successfully"));
}

#[test]
fn test_normalize_final_answer_doubled_sentences() {
    use deterministic_ai_kernel::execution::summary::normalize_final_answer;

    let text = "Task completed successfully. Task completed successfully.";
    let normalized = normalize_final_answer(text);
    assert_eq!(normalized, "Task completed successfully.");

    let glued = "Task completed successfully.Task completed successfully.";
    let normalized_glued = normalize_final_answer(glued);
    assert_eq!(normalized_glued, "Task completed successfully.");

    let text2 = "Simple text";
    assert_eq!(normalize_final_answer(text2), "Simple text");
}
