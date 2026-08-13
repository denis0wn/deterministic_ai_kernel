use anyhow::Result;

/// LLM-assisted decomposition for complex single-step tasks.
/// Only invoked in verified mode when deterministic decomposition yields 1 step.
pub async fn llm_assisted_decompose(payload: &str) -> Result<Option<Vec<String>>> {
    // Guard: only attempt if payload is long enough to warrant decomposition
    if payload.len() < 100 {
        println!(
            "observability: component=parser operation=llm_decompose_skipped \
             reason=payload_too_short payload_len={}",
            payload.len()
        );
        return Ok(None);
    }

    let system_prompt = r#"You are a task decomposition assistant. Your job is to break down a complex task into the minimum necessary number of independent, executable steps.

RULES:
- Decompose into 2-5 concrete, actionable steps
- Each step must be a complete, standalone action
- Do NOT invent steps unrelated to the original task
- Do NOT add steps the user didn't ask for
- Keep steps concise and specific
- If the task is already simple enough as one step, return it unchanged as a single-element list

OUTPUT: Return a JSON array of step strings. Example: ["step one", "step two", "step three"]"#;

    let user_prompt = format!(
        "Decompose this task into independent executable steps:\n\n{}",
        payload
    );

    let result = crate::llm::chat_structured(
        crate::model_registry::ModelPurpose::TaskPlanning,
        system_prompt,
        &user_prompt,
    )
    .await;

    match result {
        Ok(value) => {
            // Expect a JSON array of strings
            let steps = if let Some(arr) = value.as_array() {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            } else {
                // If LLM returned a single string instead of array, wrap it
                if let Some(s) = value.as_str() {
                    vec![s.trim().to_string()]
                } else {
                    println!(
                        "observability: component=parser operation=llm_decompose_skipped \
                         reason=unexpected_response_type"
                    );
                    return Ok(None);
                }
            };

            if steps.len() <= 1 {
                println!(
                    "observability: component=parser operation=llm_decompose_skipped \
                     reason=llm_returned_single steps_after=1"
                );
                return Ok(None);
            }

            println!(
                "observability: component=parser operation=llm_decompose \
                 trigger=llm_assisted payload_len={} steps_before=1 steps_after={}",
                payload.len(),
                steps.len()
            );

            Ok(Some(steps))
        }
        Err(e) => {
            println!(
                "observability: component=parser operation=llm_decompose_skipped \
                 reason=llm_error error={}",
                e
            );
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn short_payload_returns_none() {
        let result = llm_assisted_decompose("short task").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn empty_payload_returns_none() {
        let result = llm_assisted_decompose("").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn long_payload_graceful_without_llm() {
        // Without a running LLM server, this should gracefully return None
        let payload = "Make the system production-ready by adding monitoring, \
            setting up alerts, configuring auto-scaling, and writing runbooks \
            for the operations team to handle incidents.";
        let result = llm_assisted_decompose(payload).await.unwrap();
        // Should be None (LLM unavailable) or Some if LLM happens to be running
        // The key assertion is that it doesn't panic or error
        assert!(result.is_none() || result.as_ref().map_or(false, |v| v.len() > 1));
    }
}
