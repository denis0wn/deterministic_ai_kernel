#![allow(dead_code, unused)]
//! PR3+PR4: PlannerPipeline — Task → Parser → Normalizer → SemanticMapper →
//!          PlannerCritic::analyze → PlannerRecover::apply → FinalPlan
//!
//! Invariants:
//! - I1: Pipeline is a pure function — Plan = f(Task, Seed, Manifest)
//! - I2: No HashMap iteration used for ordering (Vec-based lookup only)
//! - I5: Each stage publishes an event to EventBus
//! - I6: PipelineReport produced and available for Replay storage

use crate::event_bus::EventBus;
use crate::workflow::contract::StepKind;
use crate::workflow::critic::{PlannerCritic, PlannerRecover};
use crate::workflow::plan_report::PipelineReport;
use crate::workflow::planner::{apply_semantic_bias_from_seed, parse_steps};
use crate::workflow::planner_types::{PlannerManifest, PlannerStep, StepProvenance};
use serde_json::json;

// ---------------------------------------------------------------------------
// PipelineInput / PipelineOutput
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PipelineInput {
    pub task_id: String,
    pub task_text: String,
    pub seed: u64,
    pub manifest: PlannerManifest,
}

#[derive(Debug, Clone)]
pub struct PipelineOutput {
    pub task_id: String,
    pub steps: Vec<PlannerStep>,
    pub seed: u64,
    pub manifest: PlannerManifest,
    pub report: PipelineReport,
}

// ---------------------------------------------------------------------------
// PlannerPipeline
// ---------------------------------------------------------------------------

pub struct PlannerPipeline<'a> {
    bus: &'a EventBus,
}

impl<'a> PlannerPipeline<'a> {
    pub fn new(bus: &'a EventBus) -> Self {
        Self { bus }
    }

    pub fn run(&self, input: PipelineInput) -> anyhow::Result<PipelineOutput> {
        let task_id = &input.task_id;
        let seed = input.seed;
        let manifest = &input.manifest;

        // PipelineStarted (I5)
        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "PipelineStarted",
            &json!({ "seed": seed, "planner_version": manifest.planner_version }),
        )?;

        // --- Stage 1: Parser ---
        let raw_kinds = parse_steps(&input.task_text);
        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "TaskParsed",
            &json!({ "step_count": raw_kinds.len() }),
        )?;

        // --- Stage 2: Normalizer ---
        let normalized: Vec<PlannerStep> = raw_kinds
            .into_iter()
            .map(|kind| {
                use crate::workflow::contract::Step;
                let step = Step { kind, detail: None };
                PlannerStep::from_step(step, manifest, seed).with_provenance(
                    StepProvenance::TaskParser {
                        task_id: task_id.clone(),
                    },
                )
            })
            .collect();

        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "StepsNormalized",
            &json!({ "step_count": normalized.len() }),
        )?;

        // --- Stage 3: Semantic Mapper ---
        // Vec-based lookup preserves deterministic ordering (I2 — no HashMap iteration)
        let kinds_only: Vec<StepKind> = normalized.iter().map(|ps| ps.step.kind.clone()).collect();
        let biased_kinds = apply_semantic_bias_from_seed(kinds_only, Some(seed));

        let mut normalized_owned: Vec<PlannerStep> = normalized;
        let biased: Vec<PlannerStep> = biased_kinds
            .into_iter()
            .map(|kind| {
                if let Some(pos) = normalized_owned.iter().position(|ps| ps.step.kind == kind) {
                    normalized_owned.remove(pos)
                } else {
                    use crate::workflow::contract::Step;
                    let step = Step { kind, detail: None };
                    PlannerStep::from_step(step, manifest, seed)
                }
            })
            .collect();

        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "SemanticBiasApplied",
            &json!({ "seed": seed, "step_count": biased.len() }),
        )?;

        // --- Stage 4: Critic analyze ---
        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "CriticStarted",
            &json!({ "step_count": biased.len() }),
        )?;

        let critic_report = PlannerCritic::analyze(&biased);
        let issue_count = critic_report.issues.len();

        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "CriticFinished",
            &json!({ "issue_count": issue_count, "clean": critic_report.is_clean() }),
        )?;

        // --- Stage 5: Recover ---
        let actions = PlannerRecover::plan_recovery(&critic_report);
        let recovered_flag = !actions.is_empty();
        let final_steps = PlannerRecover::apply(biased, &actions, manifest, seed);

        // --- Stage 6: PlanFinalized ---
        let report = PipelineReport::new(
            task_id.clone(),
            seed,
            manifest.clone(),
            final_steps.clone(),
            issue_count,
            recovered_flag,
        );

        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "PlanFinalized",
            &json!({
                "step_count": report.steps.len(),
                "fingerprint": report.fingerprint.0,
                "recovered": report.recovered,
            }),
        )?;

        // PipelineFinished (I5)
        self.bus.append_event(
            task_id,
            Some("pipeline"),
            "PipelineFinished",
            &json!({ "step_count": report.steps.len() }),
        )?;

        Ok(PipelineOutput {
            task_id: task_id.clone(),
            steps: final_steps,
            seed,
            manifest: manifest.clone(),
            report,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::EventBus;
    use crate::workflow::planner_types::PlannerManifest;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn tmp_db() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dak_pipeline_test_{}.db", nanos))
    }

    #[test]
    fn pipeline_publishes_all_required_events() {
        let db = tmp_db();
        let bus = EventBus::new(&db).unwrap();
        let input = PipelineInput {
            task_id: "test-pipeline-events".into(),
            task_text: "normalize output\nadd test coverage\nvalidate output".into(),
            seed: 42,
            manifest: PlannerManifest::v1(),
        };
        PlannerPipeline::new(&bus).run(input).unwrap();
        let events = bus.query("test-pipeline-events").unwrap();
        let event_types: Vec<&str> = events.iter().map(|e| e.event_type.as_str()).collect();
        for required in &[
            "PipelineStarted",
            "TaskParsed",
            "StepsNormalized",
            "SemanticBiasApplied",
            "CriticStarted",
            "CriticFinished",
            "PlanFinalized",
            "PipelineFinished",
        ] {
            assert!(
                event_types.contains(required),
                "missing event: {}",
                required
            );
        }
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn pipeline_is_deterministic_same_seed() {
        let db1 = tmp_db();
        let db2 = tmp_db();
        let bus1 = EventBus::new(&db1).unwrap();
        let bus2 = EventBus::new(&db2).unwrap();
        let input = PipelineInput {
            task_id: "det-task".into(),
            task_text: "normalize output\nadd test coverage".into(),
            seed: 7,
            manifest: PlannerManifest::v1(),
        };
        let out1 = PlannerPipeline::new(&bus1).run(input.clone()).unwrap();
        let out2 = PlannerPipeline::new(&bus2).run(input).unwrap();
        assert_eq!(
            out1.steps.iter().map(|s| &s.step.kind).collect::<Vec<_>>(),
            out2.steps.iter().map(|s| &s.step.kind).collect::<Vec<_>>(),
        );
        assert_eq!(
            out1.steps.iter().map(|s| &s.id).collect::<Vec<_>>(),
            out2.steps.iter().map(|s| &s.id).collect::<Vec<_>>(),
        );
        assert_eq!(out1.report.fingerprint, out2.report.fingerprint);
        let _ = std::fs::remove_file(&db1);
        let _ = std::fs::remove_file(&db2);
    }

    #[test]
    fn pipeline_empty_task_yields_recover_step() {
        let db = tmp_db();
        let bus = EventBus::new(&db).unwrap();
        let input = PipelineInput {
            task_id: "empty-task".into(),
            task_text: "".into(),
            seed: 0,
            manifest: PlannerManifest::v1(),
        };
        let out = PlannerPipeline::new(&bus).run(input).unwrap();
        assert!(
            !out.steps.is_empty(),
            "recovery must insert at least one step"
        );
        assert!(out.report.recovered);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn pipeline_report_fingerprint_stable() {
        let db = tmp_db();
        let bus = EventBus::new(&db).unwrap();
        let input = PipelineInput {
            task_id: "fp-task".into(),
            task_text: "normalize output\nadd test coverage".into(),
            seed: 99,
            manifest: PlannerManifest::v1(),
        };
        let out = PlannerPipeline::new(&bus).run(input).unwrap();
        let recomputed = crate::workflow::plan_report::PlanFingerprint::compute(
            &out.report.steps,
            &out.report.manifest,
        );
        assert_eq!(out.report.fingerprint, recomputed);
        let _ = std::fs::remove_file(&db);
    }
}
