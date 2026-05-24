use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::json;

pub fn seed_demo_leases(db: &str, task_id: &str) -> Result<()> {
    let conn = Connection::open(db)?;

    let current_generation: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        "INSERT OR IGNORE INTO leases
         (lease_id, task_id, step_id, worker_id, acquired_generation, expires_at_generation, state)
         VALUES (?1, ?2, ?3, 'worker-demo', ?4, ?5, 'active')",
        params![
            format!("{task_id}/step_2/lease"),
            task_id,
            "step_2",
            current_generation,
            current_generation - 1
        ],
    )?;

    println!("LEASE_SEED_OK");
    Ok(())
}

pub fn expire_leases(db: &str, task_id: &str) -> Result<()> {
    let conn = Connection::open(db)?;

    let current_generation: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT lease_id, step_id
         FROM leases
         WHERE task_id = ?1
           AND state = 'active'
           AND expires_at_generation <= ?2
         ORDER BY step_id",
    )?;

    let rows = stmt.query_map(params![task_id, current_generation], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    let expired: Vec<(String, String)> = rows.filter_map(|r| r.ok()).collect();

    for (lease_id, step_id) in &expired {
        conn.execute(
            "UPDATE leases
             SET state = 'expired'
             WHERE lease_id = ?1 AND state = 'active'",
            [lease_id],
        )?;

        let payload = json!({
            "lease_id": lease_id,
            "reason": "generation_timeout"
        });

        conn.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, 'LEASE_EXPIRED', ?5, ?6)",
            params![
                current_generation + 1,
                current_generation + 1,
                task_id,
                step_id,
                serde_json::to_string(&payload)?,
                current_generation + 1
            ],
        )?;

        conn.execute(
            "UPDATE step_status
             SET status = 'ready'
             WHERE task_id = ?1 AND step_id = ?2 AND status = 'dispatched'",
            params![task_id, step_id],
        )?;
    }

    println!("LEASE_EXPIRE_OK");
    println!("EXPIRED_LEASES: {}", expired.len());
    Ok(())
}
