//! C3: feedback-cycle event vocabulary (Layer 2 prerequisite).
//!
//! The loop itself lands with the Layer-2 POC (`LAYER2_VERIFIER_FEEDBACK_SPEC.md`);
//! these event types are the contract it will emit through the canonical event
//! bus so replay and the analyzer see every cycle. Locked now so the POC diff
//! is mechanics, not vocabulary. All payloads are kernel-derived only: failing
//! test names, hashes, indices — never model text, never test output.

/// A `tests_failed` step with known failing-test names opened a cycle.
/// Payload: `{failing_tests: [name], budget: u64}`.
pub const FEEDBACK_CYCLE_STARTED: &str = "FEEDBACK_CYCLE_STARTED";

/// One feedback attempt re-entered patch generation.
/// Payload: `{attempt: u64, prior_patch_blake3: String, failing_tests: [name]}`.
pub const FEEDBACK_ATTEMPT: &str = "FEEDBACK_ATTEMPT";

/// Budget exhausted or the model repeated an identical patch (temp-0
/// futility) — the task ends in the same honest failure as without a loop.
/// Payload: `{attempts: u64, reason: "budget_exhausted" | "identical_patch"}`.
pub const FEEDBACK_EXHAUSTED: &str = "FEEDBACK_EXHAUSTED";

/// An attempt's tests passed — the cycle converted the failure. The task
/// proceeds to validation unchanged; success is never widened.
/// Payload: `{attempt: u64}`.
pub const FEEDBACK_CONVERTED: &str = "FEEDBACK_CONVERTED";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventBus;

    /// The canonical bus accepts, persists and returns the cycle vocabulary —
    /// the channel the POC will emit through.
    #[test]
    fn feedback_events_roundtrip_through_event_bus() {
        let db = std::env::temp_dir().join(format!(
            "dak_c3_{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db_str = db.to_string_lossy().into_owned();
        let bus = EventBus::new(&db_str).unwrap();
        bus.append_event(
            "c3-task",
            Some("04_run_tests"),
            FEEDBACK_CYCLE_STARTED,
            &serde_json::json!({"failing_tests": ["test_a"], "budget": 2}),
        )
        .unwrap();
        let events = bus.query("c3-task").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, FEEDBACK_CYCLE_STARTED);
        assert!(events[0].payload.contains("test_a"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(format!("{db_str}-wal"));
        let _ = std::fs::remove_file(format!("{db_str}-shm"));
    }
}
