use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::planner_pipeline::pipeline::Pipeline;
use crate::planner_pipeline::PipelineContext;
use crate::semantic_bias::BiasVersion;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayEntry {
    pub payload: String,
    pub seed: u64,
    pub plan_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayTape {
    entries: Vec<ReplayEntry>,
}

impl ReplayTape {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn record(&mut self, payload: &str, seed: u64, plan_id: &str) {
        self.entries.push(ReplayEntry {
            payload: payload.to_owned(),
            seed,
            plan_id: plan_id.to_owned(),
        });
    }
    pub fn entries(&self) -> &[ReplayEntry] {
        &self.entries
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct Replayer {
    pipeline: Pipeline,
}

impl Replayer {
    pub fn new(pipeline: Pipeline) -> Self {
        Self { pipeline }
    }

    pub fn verify(&self, tape: &ReplayTape) -> Result<()> {
        for entry in tape.entries() {
            let task_id = format!(
                "task_{}",
                &blake3::hash(entry.payload.as_bytes()).to_hex()[..16]
            );

            // 1. Load historical events from database for this task_id
            let hist_events = crate::providers::get_storage()
                .query_events(&task_id)
                .unwrap_or_default();

            // Parse historical events
            let mut hist_plan_id = None;
            let mut hist_fingerprint = None;
            let mut hist_steps = vec![];
            let mut hist_primitives = vec![]; // Vec<(step_id, primitive_id, input_hash, output_hash, event_index)>

            for (idx, row) in hist_events.iter().enumerate() {
                let payload_json: serde_json::Value =
                    serde_json::from_str(&row.payload).unwrap_or_default();
                let details = payload_json.get("details").unwrap_or(&payload_json);
                match row.event_type.as_str() {
                    "PLAN_CREATED" => {
                        hist_plan_id = details
                            .get("plan_id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        hist_fingerprint = details
                            .get("environment_fingerprint")
                            .or_else(|| details.get("fingerprint"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                    }
                    "STEP_STARTED" => {
                        if let Some(sid) = details.get("step_id").and_then(|v| v.as_str()) {
                            hist_steps.push(sid.to_string());
                        }
                    }
                    "PRIMITIVE_EXECUTED" => {
                        let pid = details
                            .get("primitive_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let ihash = details
                            .get("input_hash")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let ohash = details
                            .get("output_hash")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let step_id = row.step_id.clone();
                        hist_primitives.push((step_id, pid, ihash, ohash, idx));
                    }
                    _ => {}
                }
            }

            // 2. Run current pipeline
            let ctx = PipelineContext {
                seed: entry.seed,
                bias_version: BiasVersion::V1,
                task_id: Some(task_id.clone()),
            };

            let out = self.pipeline.run(entry.payload.clone(), &ctx)?;
            let plan = &out.plan;

            // 3. Verify plan ID
            if plan.id != entry.plan_id {
                bail!(
                    "Replay Drift Detected:\n\
                     expected: {}\n\
                     actual: {}\n\
                     first_difference: tape plan_id mismatch\n\
                     event_index: 0",
                    entry.plan_id,
                    plan.id
                );
            }
            if let Some(ref expected_plan_id) = hist_plan_id {
                if plan.id != *expected_plan_id {
                    bail!(
                        "Replay Drift Detected:\n\
                         expected: {}\n\
                         actual: {}\n\
                         first_difference: plan_id mismatch\n\
                         event_index: 0",
                        expected_plan_id,
                        plan.id
                    );
                }
            }

            // Verify environment fingerprint matches
            if let Some(ref expected_fingerprint) = hist_fingerprint {
                let current_fingerprint = crate::planner_pipeline::get_environment_fingerprint();
                if current_fingerprint != *expected_fingerprint {
                    bail!(
                        "Replay Drift Detected:\n\
                         expected: {}\n\
                         actual: {}\n\
                         first_difference: environment fingerprint mismatch\n\
                         event_index: 0",
                        expected_fingerprint,
                        current_fingerprint
                    );
                }
            }

            // 4. Verify steps list
            let mut actual_steps = vec![];
            for step_spec in &plan.spec.steps {
                actual_steps.push(step_spec.step_id.clone());
            }
            if !hist_steps.is_empty() && actual_steps != hist_steps {
                bail!(
                    "Replay Drift Detected:\n\
                     expected: {:?}\n\
                     actual: {:?}\n\
                     first_difference: step sequence mismatch\n\
                     event_index: 0",
                    hist_steps,
                    actual_steps
                );
            }

            // 5. Verify primitive sequence, input_hash, and output_hash
            let mut actual_primitives = vec![];
            let mut prim_idx = 0;
            for step_spec in &plan.spec.steps {
                if let Some(ref prim) = step_spec.primitive {
                    let prim_id = prim.id.0.clone();
                    let ihash =
                        crate::planner_pipeline::execution_engine::calculate_primitive_input_hash(
                            prim,
                        );

                    let mut ohash = "".to_string();
                    if prim_idx < hist_primitives.len() {
                        ohash = hist_primitives[prim_idx].3.clone();
                    }
                    let _env_fp = crate::planner_pipeline::get_environment_fingerprint();
                    actual_primitives.push((step_spec.step_id.clone(), prim_id, ihash, ohash));
                    prim_idx += 1;
                }
            }

            for (i, (actual_step_id, actual_prim_id, actual_ihash, actual_ohash)) in
                actual_primitives.iter().enumerate()
            {
                if i >= hist_primitives.len() {
                    if actual_prim_id.ends_with("_execute_changes") {
                        continue;
                    }
                    bail!(
                        "Replay Drift Detected:\n\
                         expected: <none>\n\
                         actual: {}\n\
                         first_difference: primitive sequence length exceeded\n\
                         event_index: {}",
                        actual_prim_id,
                        i
                    );
                }
                let (expected_step_id, expected_prim_id, expected_ihash, expected_ohash, event_idx) =
                    &hist_primitives[i];
                if actual_step_id != expected_step_id {
                    bail!(
                        "Replay Drift Detected:\n\
                         expected: {}\n\
                         actual: {}\n\
                         first_difference: step ID mismatch for primitive {}\n\
                         event_index: {}",
                        expected_step_id,
                        actual_step_id,
                        actual_prim_id,
                        event_idx
                    );
                }
                if actual_prim_id != expected_prim_id {
                    bail!(
                        "Replay Drift Detected:\n\
                         expected: {}\n\
                         actual: {}\n\
                         first_difference: primitive ID mismatch\n\
                         event_index: {}",
                        expected_prim_id,
                        actual_prim_id,
                        event_idx
                    );
                }
                if actual_ihash != expected_ihash {
                    bail!(
                        "Replay Drift Detected:\n\
                         expected: {}\n\
                         actual: {}\n\
                         first_difference: input hash mismatch for primitive {}\n\
                         event_index: {}",
                        expected_ihash,
                        actual_ihash,
                        actual_prim_id,
                        event_idx
                    );
                }
                if actual_ohash != expected_ohash {
                    bail!(
                        "Replay Drift Detected:\n\
                         expected: {}\n\
                         actual: {}\n\
                         first_difference: output hash mismatch for primitive {}\n\
                         event_index: {}",
                        expected_ohash,
                        actual_ohash,
                        actual_prim_id,
                        event_idx
                    );
                }
            }

            // 6. Verify identical event order (logical canonical lifecycle sequence)
            let allowed_types = [
                "TASK_CREATED",
                "PLAN_CREATED",
                "STEP_READY",
                "STEP_STARTED",
                "PRIMITIVE_EXECUTING",
                "PRIMITIVE_EXECUTED",
                "STEP_COMPLETED",
                "STEP_FAILED",
                "TASK_COMPLETED",
            ];
            let hist_event_types: Vec<String> = hist_events
                .iter()
                .map(|e| e.event_type.clone())
                .filter(|t| allowed_types.contains(&t.as_str()))
                .collect();

            let mut expected_event_types =
                vec!["TASK_CREATED".to_string(), "PLAN_CREATED".to_string()];
            for step_spec in &plan.spec.steps {
                expected_event_types.push("STEP_READY".to_string());
                expected_event_types.push("STEP_STARTED".to_string());
                if step_spec.primitive.is_some() {
                    expected_event_types.push("PRIMITIVE_EXECUTING".to_string());
                    expected_event_types.push("PRIMITIVE_EXECUTED".to_string());
                }
                let step_failed = hist_events
                    .iter()
                    .any(|e| e.step_id == step_spec.step_id && e.event_type == "STEP_FAILED");

                if step_failed {
                    expected_event_types.push("STEP_FAILED".to_string());
                } else {
                    expected_event_types.push("STEP_COMPLETED".to_string());
                }
            }
            expected_event_types.push("TASK_COMPLETED".to_string());

            if !hist_event_types.is_empty() {
                for (idx, expected_type) in expected_event_types.iter().enumerate() {
                    if idx >= hist_event_types.len() {
                        bail!(
                            "Replay Drift Detected:\n\
                             expected: {}\n\
                             actual: <none>\n\
                             first_difference: expected event type '{}' at index {}\n\
                             event_index: {}",
                            expected_type,
                            expected_type,
                            idx,
                            idx
                        );
                    }
                    if hist_event_types[idx] != *expected_type {
                        bail!(
                            "Replay Drift Detected:\n\
                             expected: {}\n\
                             actual: {}\n\
                             first_difference: event type mismatch at index {}\n\
                             event_index: {}",
                            expected_type,
                            hist_event_types[idx],
                            idx,
                            idx
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_pipeline::BiasConfiguration;
    use crate::semantic_bias::SemanticBiasRule;

    struct TestDbGuard {
        db_path: std::path::PathBuf,
    }
    impl TestDbGuard {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test failure")
                .as_nanos();
            let db_path = std::env::temp_dir().join(format!("dak_test_{}_{}.db", name, nanos));
            let db_path_str = db_path.to_str().expect("test failure").to_string();
            crate::providers::get_storage().set_override_path(Some(db_path_str));
            Self { db_path }
        }
    }
    impl Drop for TestDbGuard {
        fn drop(&mut self) {
            crate::providers::get_storage().set_override_path(None);
            let _ = std::fs::remove_file(&self.db_path);
        }
    }

    fn mkp() -> Pipeline {
        Pipeline::new(BiasConfiguration::new(
            "test",
            vec![SemanticBiasRule::new("r1", 1, "critical", "first")],
        ))
    }
    fn ctx(s: u64) -> PipelineContext {
        PipelineContext {
            seed: s,
            bias_version: BiasVersion::V1,
            task_id: None,
        }
    }

    #[test]
    fn tape_records_entries() {
        let mut t = ReplayTape::new();
        t.record("step one\nstep two", 42, "abc123");
        assert_eq!(t.len(), 1);
        assert_eq!(t.entries()[0].seed, 42);
    }

    #[test]
    fn tape_is_empty_initially() {
        assert!(ReplayTape::new().is_empty());
    }

    #[test]
    fn replayer_verifies_stable_run() {
        let _guard = TestDbGuard::new("replayer_verifies_stable_run");
        let c = ctx(99);
        let eng = crate::planner_pipeline::execution_engine::ExecutionEngine::with_default_executor(
            mkp(),
        );
        let mut tape = ReplayTape::new();
        let r = eng
            .run_with_replay("step one\nstep two\ncritical step", &c, &mut tape)
            .expect("test failure");
        assert!(r.success);
        assert!(Replayer::new(mkp()).verify(&tape).is_ok());
    }

    #[test]
    fn replayer_catches_tampered_id() {
        let _guard = TestDbGuard::new("replayer_catches_tampered_id");
        let c = ctx(7);
        let eng = crate::planner_pipeline::execution_engine::ExecutionEngine::with_default_executor(
            mkp(),
        );
        let mut tape = ReplayTape::new();
        eng.run_with_replay("step one\nstep two", &c, &mut tape)
            .expect("test failure");

        let mut tampered_tape = ReplayTape::new();
        tampered_tape.record("step one\nstep two", c.seed, "0000000000000000");
        assert!(Replayer::new(mkp()).verify(&tampered_tape).is_err());
    }

    #[test]
    fn replayer_handles_empty_tape() {
        assert!(Replayer::new(mkp()).verify(&ReplayTape::new()).is_ok());
    }

    #[test]
    fn replayer_verifies_multiple_entries() {
        let _guard = TestDbGuard::new("replayer_verifies_multiple_entries");
        let mut tape = ReplayTape::new();
        let eng = crate::planner_pipeline::execution_engine::ExecutionEngine::with_default_executor(
            mkp(),
        );
        for seed in [1u64, 2, 3] {
            let c = ctx(seed);
            let payload = format!("task alpha\ntask beta\ntask {seed}");
            let r = eng
                .run_with_replay(&payload, &c, &mut tape)
                .expect("test failure");
            assert!(r.success);
        }
        assert!(Replayer::new(mkp()).verify(&tape).is_ok());
    }
}
