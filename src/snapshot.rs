use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn load_latest_snapshot(
    conn: &Connection,
    task_id: &str,
) -> Result<(i64, BTreeMap<String, String>, bool)> {
    let latest = conn.query_row(
        "SELECT payload
         FROM state_snapshots
         WHERE task_id = ?1
         ORDER BY snapshot_id DESC
         LIMIT 1",
        [task_id],
        |r| r.get::<_, String>(0),
    );

    match latest {
        Ok(payload_str) => {
            let payload: Value = serde_json::from_str(&payload_str)?;
            let last_generation = payload
                .get("last_generation")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let done = payload
                .get("done")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut steps = BTreeMap::new();
            if let Some(obj) = payload.get("steps").and_then(|v| v.as_object()) {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        steps.insert(k.clone(), s.to_string());
                    }
                }
            }

            Ok((last_generation, steps, done))
        }
        Err(_) => Ok((0, BTreeMap::new(), false)),
    }
}

pub fn rebuild_snapshot(db: &str, task_id: &str, quiet: bool) -> Result<()> {
    let conn = Connection::open(db)?;

    let (base_generation, mut steps, mut done) = load_latest_snapshot(&conn, task_id)?;

    let mut stmt = conn.prepare(
        "SELECT system_generation, step_id, event_type, payload
         FROM event_log
         WHERE task_id = ?1 AND system_generation > ?2
         ORDER BY causal_unit_id, sequence_in_unit",
    )?;

    let rows = stmt.query_map(params![task_id, base_generation], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    let mut last_generation = base_generation;

    for row in rows {
        let (generation, step_id, event_type, _payload) = row?;
        last_generation = generation;

        match event_type.as_str() {
            "STEP_COMPLETED" => {
                if let Some(step_id) = step_id {
                    steps.insert(step_id, "committed".to_string());
                }
            }
            "STEP_FAILED" => {
                if let Some(step_id) = step_id {
                    steps.insert(step_id, "rejected".to_string());
                }
            }
            "STEP_DISPATCHED" => {
                if let Some(step_id) = step_id {
                    steps
                        .entry(step_id)
                        .or_insert_with(|| "dispatched".to_string());
                }
            }
            "LEASE_ACQUIRED" => {
                if let Some(step_id) = step_id {
                    steps.entry(step_id).or_insert_with(|| "leased".to_string());
                }
            }
            "DONE" => {
                done = true;
            }
            _ => {}
        }
    }

    // Query linked semantic artifacts for this task
    let mut artifact_refs: BTreeMap<String, Value> = BTreeMap::new();
    {
        let conn2 = Connection::open(db)?;
        let mut stmt2 = conn2.prepare(
            "SELECT artifact_type, artifact_id FROM semantic_artifacts
             WHERE task_id = ?1
             ORDER BY artifact_id DESC",
        )?;
        let rows2 = stmt2.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows2 {
            let (artifact_type, artifact_id) = row?;
            artifact_refs
                .entry(artifact_type)
                .or_insert_with(|| json!(artifact_id));
        }
    }

    let state_payload = json!({
        "task_id": task_id,
        "last_generation": last_generation,
        "done": done,
        "steps": steps,
        "artifacts": artifact_refs
    });

    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let state_hash = serde_json::to_string(&state_payload)?.len() as u64;

    let payload = json!({
        "snapshot_version": 1,
        "schema_version": 1,
        "created_at": created_at,
        "state_hash": state_hash,
        "state": state_payload,
        "task_id": task_id,
        "last_generation": last_generation,
        "done": done,
        "steps": steps,
        "artifacts": artifact_refs
    });

    conn.execute(
        "INSERT INTO state_snapshots (task_id, last_generation, payload)
         VALUES (?1, ?2, ?3)",
        params![task_id, last_generation, serde_json::to_string(&payload)?],
    )?;

    if !quiet {
        println!("SNAPSHOT OK");
        println!("SNAPSHOT_BASE_GENERATION: {}", base_generation);
        println!("SNAPSHOT_GENERATION: {}", last_generation);
    }
    Ok(())
}

pub fn restore_snapshot(db: &str, task_id: &str, quiet: bool) -> Result<()> {
    let conn = Connection::open(db)?;

    let (snapshot_id, last_generation, payload): (i64, i64, String) = conn.query_row(
        "SELECT snapshot_id, last_generation, payload
         FROM state_snapshots
         WHERE task_id = ?1
         ORDER BY snapshot_id DESC
         LIMIT 1",
        [task_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;

    let payload_json: Value = serde_json::from_str(&payload)?;

    let snapshot_version = payload_json
        .get("snapshot_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    let schema_version = payload_json
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);

    if snapshot_version > 1 {
        return Err(anyhow!(
            "unsupported future snapshot_version: {}",
            snapshot_version
        ));
    }
    if schema_version > 1 {
        return Err(anyhow!(
            "unsupported future schema_version: {}",
            schema_version
        ));
    }

    if !quiet {
        println!("RESTORE OK");
        println!("SNAPSHOT_ID: {}", snapshot_id);
        println!("SNAPSHOT_GENERATION: {}", last_generation);

        if let Some(artifacts) = payload_json.get("artifacts").and_then(|v| v.as_object()) {
            for (artifact_type, artifact_id) in artifacts {
                println!("ARTIFACT_REF\t{}\t{}", artifact_type, artifact_id);
            }
        }

        println!("{}", payload);
    }
    Ok(())
}
