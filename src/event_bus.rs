use anyhow::Result;
use serde_json::Value;
use std::path::Path;

use crate::kernel_types::{ExecutionEvent, StateGraph, StateGraphEdge, StateGraphNode};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventEnvelope {
    pub event_id: String,
    pub task_id: String,
    pub execution_id: String,
    pub generation: i64,
    pub timestamp: String,
    pub event_type: String,
    pub payload_hash: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct EventRow {
    pub event_id: String,
    pub causal_unit_id: i64,
    pub sequence_in_unit: i64,
    pub task_id: String,
    pub step_id: String,
    pub event_type: String,
    pub payload: String,
}

#[derive(Debug, Clone)]
pub struct SemanticArtifactRow {
    pub artifact_id: i64,
    pub task_id: String,
    pub step_id: String,
    pub source_generation: i64,
    pub artifact_type: String,
    pub payload: String,
    #[allow(dead_code)]
    pub created_at: String,
}

#[derive(Clone)]
pub struct EventBus {
    _db_path: String,
}

impl EventBus {
    fn bind_storage(&self) {
        crate::providers::get_storage().set_override_path(Some(self._db_path.clone()));
    }

    pub fn new(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_str = db_path.as_ref().to_string_lossy().into_owned();

        let storage = crate::providers::get_storage();
        storage.set_override_path(Some(db_str.clone()));

        let conn = rusqlite::Connection::open(&db_str)?;
        conn.execute_batch(include_str!("../event_bus/schema.sql"))?;

        // Ensure the storage-backed schema/migrations are also applied on the same DB file.
        // This keeps append_semantic_artifact/list_semantic_artifacts aligned with the
        // provider implementation used by EventBus methods.
        let _ = storage.list_semantic_artifacts("__schema_probe__", None);

        Ok(Self { _db_path: db_str })
    }

    pub fn latest_generation_for_task(&self, task_id: &str) -> Result<i64> {
        self.bind_storage();
        crate::providers::get_storage().latest_generation_for_task(task_id)
    }

    #[allow(dead_code)]
    pub fn append_event(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        payload: &Value,
    ) -> Result<i64> {
        self.bind_storage();
        crate::providers::get_storage().append_event(task_id, step_id, event_type, payload)
    }

    pub fn append_semantic_artifact(
        &self,
        task_id: &str,
        step_id: &str,
        source_generation: i64,
        artifact_type: &str,
        payload: &Value,
    ) -> Result<()> {
        self.bind_storage();
        crate::providers::get_storage().append_semantic_artifact(
            task_id,
            step_id,
            source_generation,
            artifact_type,
            payload,
        )
    }

    pub fn latest_analysis_seed(
        &self,
        task_id: &str,
        step_id: Option<&str>,
    ) -> Result<Option<SemanticArtifactRow>> {
        Ok(self
            .list_semantic_artifacts(task_id, step_id)?
            .into_iter()
            .next())
    }

    pub fn list_semantic_artifacts(
        &self,
        task_id: &str,
        step_id: Option<&str>,
    ) -> Result<Vec<SemanticArtifactRow>> {
        self.bind_storage();
        crate::providers::get_storage().list_semantic_artifacts(task_id, step_id)
    }

    #[allow(dead_code)]
    pub fn commit_causal_unit(
        &self,
        task_id: &str,
        step_id: &str,
        events: Vec<(String, Value)>,
    ) -> Result<i64> {
        self.bind_storage();
        crate::providers::get_storage().commit_causal_unit(task_id, step_id, events)
    }

    #[allow(dead_code)]
    pub fn query(&self, task_id: &str) -> Result<Vec<EventRow>> {
        self.bind_storage();
        crate::providers::get_storage().query_events(task_id)
    }

    pub fn list_execution_events(&self, task_id: &str) -> Result<Vec<ExecutionEvent>> {
        self.bind_storage();
        crate::providers::get_storage().list_execution_events(task_id)
    }

    pub fn build_state_graph(&self, task_id: &str) -> Result<StateGraph> {
        let events = self.list_execution_events(task_id)?;
        let mut graph = StateGraph::default();

        for event in &events {
            graph.nodes.push(StateGraphNode {
                id: format!("node-{}", event.id),
                kind: "event".into(),
                ref_id: event.id.clone(),
            });
        }

        for pair in events.windows(2) {
            let from = format!("node-{}", pair[0].id);
            let to = format!("node-{}", pair[1].id);
            graph.edges.push(StateGraphEdge {
                from,
                to,
                relation: "observed_before".into(),
            });
        }

        Ok(graph)
    }

    pub fn save_replay_capsule(&self, capsule: &crate::kernel_types::ReplayCapsule) -> Result<()> {
        crate::providers::get_storage().save_replay_capsule(capsule)
    }

    pub fn latest_replay_capsule(
        &self,
        task_id: &str,
    ) -> Result<Option<crate::kernel_types::ReplayCapsule>> {
        crate::providers::get_storage().latest_replay_capsule(task_id)
    }

    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub fn publish_pipeline_report(
        &self,
        task_id: &str,
        plan_id: &str,
        seed: u64,
        planner_version: &str,
        steps: &[String],
        fingerprint: &str,
        elapsed_ms: u128,
        critic_passed: bool,
        warnings: &[String],
        stage_events: &[(String, String, u128)],
    ) -> Result<()> {
        use serde_json::json;

        self.append_event(
            task_id,
            None,
            "pipeline.started",
            &json!({ "seed": seed, "planner_version": planner_version }),
        )?;

        for (stage, desc, offset_ms) in stage_events {
            self.append_event(
                task_id,
                None,
                &format!("pipeline.stage.{}", stage),
                &json!({ "desc": desc, "offset_ms": offset_ms }),
            )?;
        }

        self.append_event(
            task_id,
            None,
            "pipeline.completed",
            &json!({
                "plan_id": plan_id,
                "fingerprint": fingerprint,
                "steps": steps,
                "elapsed_ms": elapsed_ms,
                "critic_passed": critic_passed,
                "warnings": warnings,
            }),
        )?;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn publish_pipeline_failed(
        &self,
        task_id: &str,
        seed: u64,
        planner_version: &str,
        reason: &str,
    ) -> Result<()> {
        use serde_json::json;
        self.append_event(
            task_id,
            None,
            "pipeline.failed",
            &json!({
                "seed": seed,
                "planner_version": planner_version,
                "reason": reason,
            }),
        )?;
        Ok(())
    }
}
