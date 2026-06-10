use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::kernel_types::{ExecutionEvent, StateGraph, StateGraphEdge, StateGraphNode, TrustContext, TrustLevel};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EventRow {
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
    conn: Arc<Mutex<Connection>>,
}

impl EventBus {
    pub fn new(db_path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(include_str!("../event_bus/schema.sql"))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    #[allow(dead_code)]
    pub fn append_event(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        event_type: &str,
        payload: &Value,
    ) -> Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let unit_gen: i64 = tx.query_row(
            "INSERT INTO generations DEFAULT VALUES RETURNING id",
            [],
            |r| r.get(0),
        )?;

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, ?7)",
            params![
                unit_gen,
                unit_gen,
                task_id,
                step_id,
                event_type,
                Self::canonical_json(payload),
                unit_gen
            ],
        )?;

        tx.commit()?;
        Ok(unit_gen)
    }

    pub fn append_semantic_artifact(
        &self,
        task_id: &str,
        step_id: &str,
        source_generation: i64,
        artifact_type: &str,
        payload: &Value,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO semantic_artifacts
             (task_id, step_id, source_generation, artifact_type, payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                task_id,
                step_id,
                source_generation,
                artifact_type,
                Self::canonical_json(payload)
            ],
        )?;
        Ok(())
    }

    pub fn latest_generation_for_task(&self, task_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let generation = conn.query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log WHERE task_id = ?1",
            [task_id],
            |r| r.get(0),
        )?;
        Ok(generation)
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
        let conn = self.conn.lock().unwrap();

        if let Some(step_id) = step_id {
            let mut stmt = conn.prepare(
                "SELECT artifact_id, task_id, step_id, source_generation, artifact_type, payload, created_at
                 FROM semantic_artifacts
                 WHERE task_id = ?1 AND step_id = ?2
                 ORDER BY source_generation DESC, artifact_id DESC",
            )?;
            let mapped = stmt.query_map(params![task_id, step_id], |r| {
                Ok(SemanticArtifactRow {
                    artifact_id: r.get(0)?,
                    task_id: r.get(1)?,
                    step_id: r.get(2)?,
                    source_generation: r.get(3)?,
                    artifact_type: r.get(4)?,
                    payload: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })?;
            let rows = mapped.collect::<std::result::Result<Vec<_>, _>>()?;
            return Ok(rows);
        }

        let mut stmt = conn.prepare(
            "SELECT artifact_id, task_id, step_id, source_generation, artifact_type, payload, created_at
             FROM semantic_artifacts
             WHERE task_id = ?1
             ORDER BY source_generation DESC, artifact_id DESC",
        )?;
        let mapped = stmt.query_map([task_id], |r| {
            Ok(SemanticArtifactRow {
                artifact_id: r.get(0)?,
                task_id: r.get(1)?,
                step_id: r.get(2)?,
                source_generation: r.get(3)?,
                artifact_type: r.get(4)?,
                payload: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?;
        let rows = mapped.collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    #[allow(dead_code)]
    pub fn commit_causal_unit(
        &self,
        task_id: &str,
        step_id: &str,
        events: Vec<(String, Value)>,
    ) -> Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let unit_gen: i64 = tx.query_row(
            "INSERT INTO generations DEFAULT VALUES RETURNING id",
            [],
            |r| r.get(0),
        )?;

        for (seq, (event_type, payload)) in events.iter().enumerate() {
            if event_type == "EFFECT_RESERVED" {
                let effect_id = payload
                    .get("effect_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing effect_id in EFFECT_RESERVED payload"))?;

                tx.execute(
                    "INSERT INTO effect_ledger
                     (effect_id, task_id, step_id, reservation_generation, state)
                     VALUES (?1, ?2, ?3, ?4, 'reserved')",
                    params![effect_id, task_id, step_id, unit_gen],
                )
                .map_err(|e| anyhow!("reserve effect {effect_id}: {e}"))?;
            }

            if event_type == "STEP_COMPLETED" {
                let effect_id = payload
                    .get("effect_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing effect_id in STEP_COMPLETED payload"))?;

                tx.execute(
                    "UPDATE effect_ledger
                     SET state = 'committed'
                     WHERE effect_id = ?1 AND task_id = ?2 AND step_id = ?3 AND state = 'reserved'",
                    params![effect_id, task_id, step_id],
                )
                .map_err(|e| anyhow!("commit effect {effect_id}: {e}"))?;
            }

            if event_type == "STEP_FAILED" {
                let effect_id = payload
                    .get("effect_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("missing effect_id in STEP_FAILED payload"))?;

                tx.execute(
                    "UPDATE effect_ledger
                     SET state = 'rejected'
                     WHERE effect_id = ?1 AND task_id = ?2 AND step_id = ?3 AND state = 'reserved'",
                    params![effect_id, task_id, step_id],
                )
                .map_err(|e| anyhow!("reject effect {effect_id}: {e}"))?;
            }

            tx.execute(
                "INSERT INTO event_log
                 (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    unit_gen,
                    unit_gen,
                    seq as i64,
                    task_id,
                    step_id,
                    event_type,
                    Self::canonical_json(payload),
                    unit_gen
                ],
            )
            .map_err(|e| anyhow!("insert event seq={seq}: {e}"))?;
        }

        tx.commit()?;
        Ok(unit_gen)
    }

    #[allow(dead_code)]
    pub fn query(&self, task_id: &str) -> Result<Vec<EventRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload
             FROM event_log
             WHERE task_id = ?1
             ORDER BY causal_unit_id, sequence_in_unit",
        )?;

        let rows = stmt.query_map([task_id], |r| {
            Ok(EventRow {
                causal_unit_id: r.get(0)?,
                sequence_in_unit: r.get(1)?,
                task_id: r.get(2)?,
                step_id: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                event_type: r.get(4)?,
                payload: r.get(5)?,
            })
        })?;

        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }


    pub fn list_execution_events(&self, task_id: &str) -> Result<Vec<ExecutionEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, event_type, payload              FROM event_log              WHERE task_id = ?1              ORDER BY causal_unit_id, sequence_in_unit, id"
        )?;

        let mapped = stmt.query_map([task_id], |r| {
            let id: i64 = r.get(0)?;
            let task_id: String = r.get(1)?;
            let event_type: String = r.get(2)?;
            let payload_raw: String = r.get(3)?;
            let payload: Value = serde_json::from_str(&payload_raw).unwrap_or(Value::String(payload_raw));

            Ok(ExecutionEvent {
                id: format!("evt-{}", id),
                task_id,
                timestamp: "event_log".to_string(),
                event_type,
                payload,
                caused_by: None,
                trust_context: TrustContext {
                    source: "event_bus".into(),
                    trust_level: TrustLevel::High,
                    verification_status: "recorded".into(),
                    policy_version: "v1".into(),
                },
            })
        })?;

        Ok(mapped.collect::<std::result::Result<Vec<_>, _>>()?)
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
        let conn = self.conn.lock().unwrap();
        let payload = serde_json::to_string(capsule)?;
        conn.execute(
            "INSERT OR REPLACE INTO replay_capsules (capsule_id, task_id, created_at, payload) VALUES (?1, ?2, ?3, ?4)",
            params![capsule.capsule_id, capsule.execution_id, capsule.created_at, payload],
        )?;
        Ok(())
    }

    
    pub fn latest_replay_capsule(
        &self,
        task_id: &str,
    ) -> Result<Option<crate::kernel_types::ReplayCapsule>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT payload
             FROM replay_capsules
             WHERE task_id = ?1
             ORDER BY created_at DESC, capsule_id DESC
             LIMIT 1"
        )?;

        let row: Option<String> = stmt
            .query_row([task_id], |r| r.get::<_, String>(0))
            .optional()?;

        match row {
            Some(payload) => {
                let capsule = serde_json::from_str::<crate::kernel_types::ReplayCapsule>(&payload)?;
                Ok(Some(capsule))
            }
            None => Ok(None),
        }
    }

    fn canonical_json(value: &Value) -> String {
        let mut ordered = BTreeMap::new();
        if let Value::Object(map) = value {
            for (k, v) in map {
                ordered.insert(k.clone(), v.clone());
            }
            serde_json::to_string(&ordered).unwrap()
        } else {
            serde_json::to_string(value).unwrap()
        }
    }
}
