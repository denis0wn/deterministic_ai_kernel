use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

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

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn canonicalize(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut ordered: BTreeMap<_, _> = BTreeMap::new();
                for (k, v) in map {
                    ordered.insert(k.clone(), Self::canonicalize(v));
                }
                serde_json::to_value(ordered).unwrap()
            }
            Value::Array(arr) => Value::Array(arr.iter().map(Self::canonicalize).collect()),
            _ => value.clone(),
        }
    }

    fn canonical_json(val: &Value) -> String {
        serde_json::to_string(&Self::canonicalize(val)).unwrap()
    }
}
