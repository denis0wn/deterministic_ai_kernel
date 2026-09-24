use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::kernel_types::{
    AILifecycleEvent, AIRequest, AIResponse, AITrace, ExecutionEvent, StateGraph, StateGraphEdge,
    StateGraphNode, TrustContext, TrustLevel,
};

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

    pub fn append_ai_request(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        request: &AIRequest,
        trace: &AITrace,
    ) -> Result<i64> {
        let payload = serde_json::json!({
            "kind": "AI_REQUEST",
            "request": request,
            "trace": trace,
        });
        self.append_event(task_id, step_id, "AI_REQUEST", &payload)
    }

    pub fn append_ai_response(
        &self,
        task_id: &str,
        step_id: Option<&str>,
        response: &AIResponse,
        trace: &AITrace,
    ) -> Result<i64> {
        let payload = serde_json::json!({
            "kind": "AI_RESPONSE",
            "response": response,
            "trace": trace,
        });
        self.append_event(task_id, step_id, "AI_RESPONSE", &payload)
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
            let payload: Value =
                serde_json::from_str(&payload_raw).unwrap_or(Value::String(payload_raw));

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
             LIMIT 1",
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

pub fn stable_event_hash(input: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    for b in input.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{:016x}", h)
}

impl EventBus {
    #[allow(dead_code)]
    pub fn record_ai_event(
        &self,
        task_id: &str,
        event: &AILifecycleEvent,
        trace: Option<&AITrace>,
    ) -> Result<ExecutionEvent> {
        let payload = serde_json::json!({
            "ai_event": event,
            "trace": trace,
        });

        let execution_event = ExecutionEvent {
            id: format!(
                "ai-{}",
                stable_event_hash(&serde_json::to_string(&payload)?)
            ),
            task_id: task_id.to_string(),
            timestamp: format!(
                "{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or_default()
            ),
            event_type: match event {
                AILifecycleEvent::InvocationStarted { .. } => "AIInvocationStarted",
                AILifecycleEvent::ChunkProduced { .. } => "AIChunkProduced",
                AILifecycleEvent::InvocationCompleted { .. } => "AIInvocationCompleted",
                AILifecycleEvent::InvocationFailed { .. } => "AIInvocationFailed",
            }
            .to_string(),
            payload,
            caused_by: None,
            trust_context: TrustContext {
                source: "ai_worker".to_string(),
                trust_level: TrustLevel::Medium,
                verification_status: "recorded".to_string(),
                policy_version: "phase1".to_string(),
            },
        };

        Ok(execution_event)
    }

    #[allow(dead_code)]
    pub fn replay_ai_event(&self, event: &ExecutionEvent) -> Result<AILifecycleEvent> {
        let ai_event = event
            .payload
            .get("ai_event")
            .ok_or_else(|| anyhow!("missing ai_event payload"))?
            .clone();

        let decoded: AILifecycleEvent = serde_json::from_value(ai_event)?;
        Ok(decoded)
    }
}

#[cfg(test)]
mod ai_event_tests {
    use super::*;
    use crate::kernel_types::{AILifecycleEvent, AITrace};

    fn sample_trace() -> AITrace {
        AITrace {
            model_id: "model-x".to_string(),
            model_hash: "hash-model".to_string(),
            prompt_hash: "hash-prompt".to_string(),
            sampling_config: "temperature=0".to_string(),
            timestamp: 123456789,
            output_hash: "hash-output".to_string(),
        }
    }

    #[test]
    fn test_ai_invocation_emits_started_event() {
        let bus = EventBus::new(std::env::temp_dir().join("event_bus_ai_test_started.sqlite"))
            .expect("event bus");
        let evt = AILifecycleEvent::InvocationStarted {
            request_id: "req-1".to_string(),
            model_id: "model-x".to_string(),
            trace_id: "trace-1".to_string(),
        };

        let recorded = bus
            .record_ai_event("task-1", &evt, Some(&sample_trace()))
            .expect("record event");

        assert_eq!(recorded.task_id, "task-1");
        assert_eq!(recorded.event_type, "AIInvocationStarted");
        assert!(recorded.payload.get("ai_event").is_some());
    }

    #[test]
    fn test_ai_invocation_replay_roundtrip() {
        let bus = EventBus::new(std::env::temp_dir().join("event_bus_ai_test_replay.sqlite"))
            .expect("event bus");
        let evt = AILifecycleEvent::InvocationCompleted {
            request_id: "req-2".to_string(),
            output_hash: "out-1".to_string(),
            duration_ms: 42,
        };

        let recorded = bus
            .record_ai_event("task-2", &evt, Some(&sample_trace()))
            .expect("record event");

        let replayed = bus.replay_ai_event(&recorded).expect("replay event");
        assert_eq!(replayed, evt);
    }

    #[test]
    fn test_ai_trace_is_deterministic() {
        let trace_a = sample_trace();
        let trace_b = sample_trace();

        let a = stable_event_hash(&serde_json::to_string(&trace_a).unwrap());
        let b = stable_event_hash(&serde_json::to_string(&trace_b).unwrap());

        assert_eq!(a, b);
    }
}

#[cfg(test)]
mod ai_helper_usage_tests {
    use super::*;
    use crate::ai::protocol::{
        AIInput, AIRequest, AIResponse, GenerationConfig, Modality, ResponseChunk, TraceContext,
    };
    use crate::ai::trace::AITrace;
    use crate::kernel_types::{AILifecycleEvent, ExecutionEvent};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("{name}-{nanos}.db"))
            .display()
            .to_string()
    }

    fn sample_request() -> AIRequest {
        AIRequest {
            request_id: "req-evt".to_string(),
            workflow_id: "wf-evt".to_string(),
            modality: Modality::Text,
            input: AIInput::Text {
                prompt: "hello".to_string(),
            },
            input_artifacts: Vec::new(),
            prompt: Some("hello".to_string()),
            model: Some("model-x".to_string()),
            generation_config: GenerationConfig::default(),
            trace_context: TraceContext::default(),
            deterministic: true,
        }
    }

    fn sample_response() -> AIResponse {
        AIResponse {
            request_id: "req-evt".to_string(),
            model_id: "model-x".to_string(),
            output_text: "world".to_string(),
            finish_reason: Some("stop".to_string()),
            chunks: vec![ResponseChunk {
                sequence: 0,
                text: "world".to_string(),
                done: true,
            }],
            generated_artifacts: Vec::new(),
            execution_metadata: BTreeMap::new(),
        }
    }

    fn sample_trace() -> AITrace {
        AITrace {
            request_id: "req-evt".to_string(),
            workflow_id: "wf-evt".to_string(),
            model_id: "model-x".to_string(),
            model_version: None,
            model_hash: stable_event_hash("model-x"),
            prompt_hash: stable_event_hash("hello"),
            input_artifact_hashes: Vec::new(),
            generation_config_hash: stable_event_hash("{}"),
            sampling_config: "temperature_milli=0".to_string(),
            seed: Some(0),
            timestamp: 0,
            output_hash: stable_event_hash("world"),
        }
    }

    #[test]
    fn stable_event_hash_is_deterministic() {
        let a = stable_event_hash("alpha");
        let b = stable_event_hash("alpha");
        let c = stable_event_hash("beta");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn record_and_replay_ai_helpers_are_exercised() -> Result<()> {
        let db = temp_db_path("event-bus-ai-helpers");
        let bus = EventBus::new(&db)?;

        let request = sample_request();
        let _response = sample_response();
        let trace = sample_trace();

        let started = AILifecycleEvent::InvocationStarted {
            request_id: request.request_id.clone(),
            model_id: trace.model_id.clone(),
            trace_id: trace.output_hash.clone(),
        };
        let completed = AILifecycleEvent::InvocationCompleted {
            request_id: request.request_id.clone(),
            output_hash: trace.output_hash.clone(),
            duration_ms: 0,
        };

        let event_trace = crate::kernel_types::AITrace {
            model_id: trace.model_id.clone(),
            model_hash: trace.model_hash.clone(),
            prompt_hash: trace.prompt_hash.clone(),
            sampling_config: trace.sampling_config.clone(),
            timestamp: trace.timestamp,
            output_hash: trace.output_hash.clone(),
        };

        bus.record_ai_event("task-evt", &started, Some(&event_trace))?;
        bus.record_ai_event("task-evt", &completed, Some(&event_trace))?;

        let started_exec = ExecutionEvent {
            id: "e1".to_string(),
            task_id: "task-evt".to_string(),
            timestamp: "0".to_string(),
            event_type: "AIInvocationStarted".to_string(),
            payload: json!({
                "ai_event": started,
                "trace": trace.clone(),
            }),
            caused_by: None,
            trust_context: TrustContext {
                source: "ai_worker".to_string(),
                trust_level: TrustLevel::Medium,
                verification_status: "recorded".to_string(),
                policy_version: "phase1".to_string(),
            },
        };

        let completed_exec = ExecutionEvent {
            id: "e2".to_string(),
            task_id: "task-evt".to_string(),
            timestamp: "0".to_string(),
            event_type: "AIInvocationCompleted".to_string(),
            payload: json!({
                "ai_event": completed,
                "trace": trace,
            }),
            caused_by: None,
            trust_context: TrustContext {
                source: "ai_worker".to_string(),
                trust_level: TrustLevel::Medium,
                verification_status: "recorded".to_string(),
                policy_version: "phase1".to_string(),
            },
        };

        match bus.replay_ai_event(&started_exec)? {
            AILifecycleEvent::InvocationStarted { .. } => {}
            _ => panic!("expected started event"),
        }

        match bus.replay_ai_event(&completed_exec)? {
            AILifecycleEvent::InvocationCompleted { .. } => {}
            _ => panic!("expected completed event"),
        }

        Ok(())
    }
}
