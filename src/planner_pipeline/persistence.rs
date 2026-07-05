use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result};

use crate::planner_pipeline::execution_engine::ExecutionReport;
use crate::planner_pipeline::replay::ReplayTape;

// ── Store config ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StoreConfig {
    pub base_dir: PathBuf,
}

impl StoreConfig {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }
    pub fn tape_path(&self) -> PathBuf { self.base_dir.join("tape.json") }
    pub fn runs_dir(&self)  -> PathBuf { self.base_dir.join("runs") }
    pub fn run_path(&self, plan_id: &str) -> PathBuf {
        self.runs_dir().join(format!("{plan_id}.json"))
    }
}

// ── PersistenceStore ──────────────────────────────────────────────────────────

pub struct PersistenceStore {
    config: StoreConfig,
}

impl PersistenceStore {
    pub fn new(config: StoreConfig) -> Result<Self> {
        fs::create_dir_all(config.runs_dir()).context("create runs dir")?;
        Ok(Self { config })
    }

    pub fn save_report(&self, report: &ExecutionReport) -> Result<()> {
        let path = self.config.run_path(&report.plan_id);
        let json = serde_json::to_string_pretty(report)
            .context("serialize ExecutionReport")?;
        fs::write(&path, json)
            .with_context(|| format!("write report to {path:?}"))
    }

    pub fn save_tape(&self, tape: &ReplayTape) -> Result<()> {
        let path = self.config.tape_path();
        let json = serde_json::to_string_pretty(tape)
            .context("serialize ReplayTape")?;
        fs::write(&path, json)
            .with_context(|| format!("write tape to {path:?}"))
    }

    pub fn load_tape(&self) -> Result<ReplayTape> {
        let path = self.config.tape_path();
        if !path.exists() { return Ok(ReplayTape::new()); }
        let json = fs::read_to_string(&path)
            .with_context(|| format!("read tape from {path:?}"))?;
        serde_json::from_str(&json).context("deserialize ReplayTape")
    }

    pub fn load_report(&self, plan_id: &str) -> Result<ExecutionReport> {
        let path = self.config.run_path(plan_id);
        let json = fs::read_to_string(&path)
            .with_context(|| format!("read report {plan_id}"))?;
        serde_json::from_str(&json).context("deserialize ExecutionReport")
    }

    pub fn list_runs(&self) -> Result<Vec<String>> {
        let dir = self.config.runs_dir();
        let mut ids = vec![];
        for entry in fs::read_dir(&dir).context("read runs dir")? {
            let name = entry?.file_name();
            let s = name.to_string_lossy().to_string();
            if s.ends_with(".json") {
                ids.push(s.trim_end_matches(".json").to_owned());
            }
        }
        ids.sort();
        Ok(ids)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner_pipeline::execution_engine::{ExecutionReport, StepResult, StepStatus};
    use crate::planner_pipeline::replay::ReplayTape;

    fn tmp_store() -> (PersistenceStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = PersistenceStore::new(StoreConfig::new(dir.path())).unwrap();
        (store, dir)
    }

    fn fake_report(plan_id: &str, descs: &[&str]) -> ExecutionReport {
        ExecutionReport {
            plan_id: plan_id.to_owned(),
            seed: 42,
            steps: descs.iter().enumerate().map(|(i, d)| StepResult {
                index: i,
                description: d.to_string(),
                status: StepStatus::Ok,
                duration_ms: 1,
            }).collect(),
            total_duration_ms: descs.len() as u64,
            success: true,
        }
    }

    #[test]
    fn save_and_load_report() {
        let (store, _dir) = tmp_store();
        let r = fake_report("abc1234567890123", &["step one", "step two"]);
        store.save_report(&r).unwrap();
        let loaded = store.load_report("abc1234567890123").unwrap();
        assert_eq!(loaded.plan_id, r.plan_id);
        assert_eq!(loaded.steps.len(), 2);
        assert!(loaded.success);
    }

    #[test]
    fn save_and_load_tape() {
        let (store, _dir) = tmp_store();
        let mut tape = ReplayTape::new();
        tape.record("step one\nstep two", 42, "abc1234567890123");
        store.save_tape(&tape).unwrap();
        let loaded = store.load_tape().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.entries()[0].seed, 42);
    }

    #[test]
    fn load_tape_returns_empty_when_absent() {
        let (store, _dir) = tmp_store();
        let tape = store.load_tape().unwrap();
        assert!(tape.is_empty());
    }

    #[test]
    fn list_runs_returns_saved_ids() {
        let (store, _dir) = tmp_store();
        store.save_report(&fake_report("plan0000000000001", &["a"])).unwrap();
        store.save_report(&fake_report("plan0000000000002", &["b"])).unwrap();
        let ids = store.list_runs().unwrap();
        assert_eq!(ids, vec!["plan0000000000001", "plan0000000000002"]);
    }

    #[test]
    fn load_missing_report_errors() {
        let (store, _dir) = tmp_store();
        assert!(store.load_report("nonexistent000000").is_err());
    }

    #[test]
    fn save_report_is_idempotent() {
        let (store, _dir) = tmp_store();
        let r = fake_report("idem000000000000", &["x"]);
        store.save_report(&r).unwrap();
        store.save_report(&r).unwrap(); // second write must not fail
        let loaded = store.load_report("idem000000000000").unwrap();
        assert_eq!(loaded.plan_id, r.plan_id);
    }
}
