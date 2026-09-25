use crate::execution::runtime::ExecutionReceipt;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeterministicSummary {
    pub task_id: String,
    pub status: String, // "success/committed" or "failed"
    pub committed_steps: usize,
    pub failed_attempts: usize,
    pub retry_count: usize,
    pub replay_valid: bool,
    pub committed_effects: usize,
    pub rejected_effects: usize,
}

impl DeterministicSummary {
    pub fn build(receipt: &ExecutionReceipt) -> Result<Self> {
        let storage = crate::providers::get_storage();
        let status_map = storage.get_current_status_map(&receipt.task_id)?;
        let (committed_effects, rejected_effects) = storage.get_effect_counts(&receipt.task_id)?;

        // Rule for status:
        // if all step statuses in status_map are committed and replay_validation == true, then success/committed
        let all_committed = !status_map.is_empty() && status_map.values().all(|s| s == "committed");
        let status = if all_committed && receipt.replay_validation {
            "success/committed".to_string()
        } else {
            "failed".to_string()
        };

        Ok(Self {
            task_id: receipt.task_id.clone(),
            status,
            committed_steps: receipt.completed_steps,
            failed_attempts: receipt.failed_attempts,
            retry_count: receipt.retry_count,
            replay_valid: receipt.replay_validation,
            committed_effects,
            rejected_effects,
        })
    }
}

pub fn final_answer_matches_summary(text: &str, summary: &DeterministicSummary) -> bool {
    let lower = text.to_lowercase();

    // Contradiction case 1: summary says committed/success, but text contains failed, failure, unsuccessful, terminal error
    if summary.status == "success/committed" {
        for word in &["failed", "failure", "unsuccessful", "terminal error"] {
            if lower.contains(word) {
                return false;
            }
        }
    }

    // Contradiction case 2: summary says replay valid, but text contains corruption or claims replay is invalid/failed
    if summary.replay_valid
        && (lower.contains("corruption")
            || (lower.contains("replay") && lower.contains("invalid"))
            || (lower.contains("replay") && lower.contains("failed")))
    {
        return false;
    }

    true
}

pub fn normalize_final_answer(text: &str) -> String {
    let trimmed = text.trim();
    let mut spaced = String::new();
    let chars: Vec<char> = trimmed.chars().collect();
    if !chars.is_empty() {
        for i in 0..chars.len() - 1 {
            spaced.push(chars[i]);
            if chars[i] == '.' && chars[i + 1].is_ascii_uppercase() {
                spaced.push(' ');
            }
        }
        spaced.push(chars[chars.len() - 1]);
    }
    let normalized_text = spaced.trim().to_string();

    let words: Vec<&str> = normalized_text.split_whitespace().collect();
    if !words.is_empty() && words.len().is_multiple_of(2) {
        let half_words = words.len() / 2;
        if words[..half_words] == words[half_words..] {
            return words[..half_words].join(" ");
        }
    }
    normalized_text
}

pub fn format_deterministic_fallback(summary: &DeterministicSummary) -> String {
    let status_str = if summary.status == "success/committed" {
        "completed successfully"
    } else {
        "failed"
    };
    let replay_str = if summary.replay_valid {
        "valid"
    } else {
        "invalid"
    };
    format!(
        "Workflow {}. Completed steps: {}. Retries: {}. Replay: {}.",
        status_str, summary.committed_steps, summary.retry_count, replay_str
    )
}

pub fn calculate_receipt_hash(receipt: &ExecutionReceipt) -> String {
    let mut map = std::collections::BTreeMap::new();
    map.insert("task_id", serde_json::json!(receipt.task_id));
    map.insert("status", serde_json::json!(receipt.status));
    map.insert(
        "completed_steps",
        serde_json::json!(receipt.completed_steps),
    );
    map.insert(
        "failed_attempts",
        serde_json::json!(receipt.failed_attempts),
    );
    map.insert("retry_count", serde_json::json!(receipt.retry_count));
    map.insert(
        "recovery_events",
        serde_json::json!(receipt.recovery_events),
    );
    map.insert("tool_calls", serde_json::json!(receipt.tool_calls));
    map.insert("llm_calls", serde_json::json!(receipt.llm_calls));
    map.insert("artifacts", serde_json::json!(receipt.artifacts));
    map.insert(
        "replay_validation",
        serde_json::json!(receipt.replay_validation),
    );
    map.insert("wall_clock_ms", serde_json::json!(receipt.wall_clock_ms));
    map.insert("planner_ms", serde_json::json!(receipt.planner_ms));
    map.insert("compiler_ms", serde_json::json!(receipt.compiler_ms));
    map.insert("scheduler_ms", serde_json::json!(receipt.scheduler_ms));
    map.insert("execution_ms", serde_json::json!(receipt.execution_ms));
    map.insert("recovery_ms", serde_json::json!(receipt.recovery_ms));
    map.insert("replay_ms", serde_json::json!(receipt.replay_ms));

    let json_str = serde_json::to_string(&map).unwrap_or_default();
    blake3::hash(json_str.as_bytes()).to_hex().to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedFinalAnswer {
    pub receipt_hash: String,
    pub deterministic_summary: DeterministicSummary,
    pub validator_passed: bool,
    pub generator_version: String,
}

pub async fn render_final_answer_from_summary(summary: &DeterministicSummary) -> Result<String> {
    let mut attempt = 0;
    let mut human_summary = String::new();
    let mut passed = false;

    let summary_json = serde_json::to_string_pretty(summary)?;

    while attempt < 3 {
        let warning_message = if attempt == 0 {
            None
        } else {
            Some("Your previous response contradicted the summary. Ensure you do NOT use the words 'failed' or 'failure' if the status is success, and do NOT mention invalid replay if replay is valid.")
        };

        let response = crate::llm::paraphrase_summary(&summary_json, warning_message).await?;
        human_summary = normalize_final_answer(&response);

        if final_answer_matches_summary(&human_summary, summary) {
            passed = true;
            break;
        } else {
            let storage = crate::providers::get_storage();
            let _ = storage.append_event(
                &summary.task_id,
                Some("final_answer_publisher"),
                "HUMAN_SUMMARY_REJECTED",
                &serde_json::json!({
                    "attempt": attempt,
                    "rejected_text": human_summary,
                }),
            );
        }
        attempt += 1;
    }

    if !passed {
        return Err(anyhow::anyhow!("FINAL_ANSWER_CONTRADICTS_RECEIPT"));
    }

    Ok(human_summary)
}
