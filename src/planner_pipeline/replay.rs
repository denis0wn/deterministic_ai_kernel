use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::planner_pipeline::PipelineContext;
use crate::planner_pipeline::pipeline::Pipeline;
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
    pub fn new() -> Self { Self::default() }
    pub fn record(&mut self, payload: &str, seed: u64, plan_id: &str) {
        self.entries.push(ReplayEntry {
            payload: payload.to_owned(), seed, plan_id: plan_id.to_owned(),
        });
    }
    pub fn entries(&self) -> &[ReplayEntry] { &self.entries }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

pub struct Replayer { pipeline: Pipeline }

impl Replayer {
    pub fn new(pipeline: Pipeline) -> Self { Self { pipeline } }

    pub fn verify(&self, tape: &ReplayTape) -> Result<()> {
        for entry in tape.entries() {
            let ctx = PipelineContext { seed: entry.seed, bias_version: BiasVersion::V1 };
            let report = self.pipeline.run(entry.payload.clone(), &ctx)?;
            if report.plan.id != entry.plan_id {
                bail!("Replay mismatch: expected id={} got id={}", entry.plan_id, report.plan.id);
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

    fn mkp() -> Pipeline {
        Pipeline::new(BiasConfiguration::new(
            "test", vec![SemanticBiasRule::new("r1", 1, "critical", "first")],
        ))
    }
    fn ctx(s: u64) -> PipelineContext { PipelineContext { seed: s, bias_version: BiasVersion::V1 } }

    #[test]
    fn tape_records_entries() {
        let mut t = ReplayTape::new();
        t.record("step one\nstep two", 42, "abc123");
        assert_eq!(t.len(), 1);
        assert_eq!(t.entries()[0].seed, 42);
    }

    #[test]
    fn tape_is_empty_initially() { assert!(ReplayTape::new().is_empty()); }

    #[test]
    fn replayer_verifies_stable_run() {
        let c = ctx(99);
        let r = mkp().run("step one\nstep two\ncritical step", &c).unwrap();
        let mut tape = ReplayTape::new();
        tape.record("step one\nstep two\ncritical step", c.seed, &r.plan.id);
        assert!(Replayer::new(mkp()).verify(&tape).is_ok());
    }

    #[test]
    fn replayer_catches_tampered_id() {
        let c = ctx(7);
        mkp().run("step one\nstep two", &c).unwrap();
        let mut tape = ReplayTape::new();
        tape.record("step one\nstep two", c.seed, "0000000000000000");
        assert!(Replayer::new(mkp()).verify(&tape).is_err());
    }

    #[test]
    fn replayer_handles_empty_tape() {
        assert!(Replayer::new(mkp()).verify(&ReplayTape::new()).is_ok());
    }

    #[test]
    fn replayer_verifies_multiple_entries() {
        let mut tape = ReplayTape::new();
        for seed in [1u64, 2, 3] {
            let c = ctx(seed);
            let r = mkp().run(format!("task alpha\ntask beta\ntask {seed}"), &c).unwrap();
            tape.record(&format!("task alpha\ntask beta\ntask {seed}"), seed, &r.plan.id);
        }
        assert!(Replayer::new(mkp()).verify(&tape).is_ok());
    }
}
