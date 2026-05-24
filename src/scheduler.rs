use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::collections::BTreeMap;

fn step_key(step: &str) -> i64 {
    step.trim_start_matches("step_").parse::<i64>().unwrap_or(0)
}

fn terminal(status: &str) -> bool {
    status == "committed" || status == "rejected"
}

pub fn seed_dependencies(db: &str, task_id: &str) -> Result<()> {
    let conn = Connection::open(db)?;

    for (step, dep) in [("step_1", "step_0"), ("step_2", "step_1")] {
        conn.execute(
            "INSERT OR IGNORE INTO step_dependencies (task_id, step_id, depends_on_step_id)
             VALUES (?1, ?2, ?3)",
            params![task_id, step, dep],
        )?;
    }

    for step in ["step_0", "step_1", "step_2"] {
        conn.execute(
            "INSERT OR IGNORE INTO step_status (task_id, step_id, status)
             VALUES (?1, ?2, 'pending')",
            params![task_id, step],
        )?;
    }

    Ok(())
}

pub fn reconcile(db: &str, task_id: &str) -> Result<()> {
    let conn = Connection::open(db)?;
    seed_dependencies(db, task_id)?;

    conn.execute(
        "UPDATE step_status SET status = 'pending' WHERE task_id = ?1",
        [task_id],
    )?;

    let mut stmt = conn.prepare(
        "SELECT step_id, event_type
         FROM event_log
         WHERE task_id = ?1 AND step_id IS NOT NULL
         ORDER BY causal_unit_id, sequence_in_unit, id",
    )?;

    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        let (step_id, event_type) = row;

        match event_type.as_str() {
            "STEP_COMPLETED" => {
                conn.execute(
                    "UPDATE step_status SET status = 'committed'
                     WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                )?;
                conn.execute(
                    "UPDATE leases
                     SET state = 'completed'
                     WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                    params![task_id, step_id],
                )?;
            }
            "STEP_FAILED" => {
                conn.execute(
                    "UPDATE step_status SET status = 'rejected'
                     WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                )?;
                conn.execute(
                    "UPDATE leases
                     SET state = 'released'
                     WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                    params![task_id, step_id],
                )?;
            }
            "STEP_DISPATCHED" => {
                let current: String = conn.query_row(
                    "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                    |r| r.get(0),
                )?;
                if !terminal(&current) {
                    conn.execute(
                        "UPDATE step_status SET status = 'dispatched'
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                    )?;
                }
            }
            "LEASE_EXPIRED" => {
                let current: String = conn.query_row(
                    "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                    params![task_id, step_id],
                    |r| r.get(0),
                )?;
                if !terminal(&current) {
                    conn.execute(
                        "UPDATE step_status SET status = 'pending'
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                    )?;
                    conn.execute(
                        "UPDATE leases
                         SET state = 'expired'
                         WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                        params![task_id, step_id],
                    )?;
                }
            }
            _ => {}
        }
    }

    let mut all_steps: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT step_id
             FROM step_status
             WHERE task_id = ?1
             ORDER BY step_id",
        )?;
        let rows = stmt.query_map([task_id], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    all_steps.sort_by_key(|s| step_key(s));

    for step_id in &all_steps {
        let current: String = conn.query_row(
            "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if terminal(&current) || current == "dispatched" || current == "ready" {
            continue;
        }

        let dep_total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM step_dependencies
             WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if dep_total == 0 {
            conn.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
            continue;
        }

        let dep_satisfied: i64 = conn.query_row(
            "SELECT COUNT(*)
             FROM step_dependencies d
             JOIN step_status s
               ON s.task_id = d.task_id
              AND s.step_id = d.depends_on_step_id
             WHERE d.task_id = ?1
               AND d.step_id = ?2
               AND s.status = 'committed'",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if dep_satisfied == dep_total {
            conn.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
        }
    }

    let mut status: BTreeMap<String, String> = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT step_id, status
         FROM step_status
         WHERE task_id = ?1
         ORDER BY step_id",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        status.insert(row.0, row.1);
    }

    println!("RECONCILE OK");
    println!("STEP_STATUS: {:?}", status);
    Ok(())
}

pub fn schedule(db: &str, task_id: &str) -> Result<()> {
    let mut conn = Connection::open(db)?;
    seed_dependencies(db, task_id)?;
    let tx = conn.transaction()?;

    tx.execute(
        "UPDATE step_status SET status = 'pending' WHERE task_id = ?1 AND status NOT IN ('committed','rejected')",
        [task_id],
    )?;

    {
        let mut stmt = tx.prepare(
            "SELECT step_id, event_type
             FROM event_log
             WHERE task_id = ?1 AND step_id IS NOT NULL
             ORDER BY causal_unit_id, sequence_in_unit, id",
        )?;
        let rows = stmt.query_map([task_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;

        for row in rows.filter_map(|r| r.ok()) {
            let (step_id, event_type) = row;

            match event_type.as_str() {
                "STEP_COMPLETED" => {
                    tx.execute(
                        "UPDATE step_status SET status = 'committed'
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                    )?;
                    tx.execute(
                        "UPDATE leases
                         SET state = 'completed'
                         WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                        params![task_id, step_id],
                    )?;
                }
                "STEP_FAILED" => {
                    tx.execute(
                        "UPDATE step_status SET status = 'rejected'
                         WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                    )?;
                    tx.execute(
                        "UPDATE leases
                         SET state = 'released'
                         WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                        params![task_id, step_id],
                    )?;
                }
                "STEP_DISPATCHED" => {
                    let current: String = tx.query_row(
                        "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                        |r| r.get(0),
                    )?;
                    if !terminal(&current) {
                        tx.execute(
                            "UPDATE step_status SET status = 'dispatched'
                             WHERE task_id = ?1 AND step_id = ?2",
                            params![task_id, step_id],
                        )?;
                    }
                }
                "LEASE_EXPIRED" => {
                    let current: String = tx.query_row(
                        "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
                        params![task_id, step_id],
                        |r| r.get(0),
                    )?;
                    if !terminal(&current) {
                        tx.execute(
                            "UPDATE step_status SET status = 'pending'
                             WHERE task_id = ?1 AND step_id = ?2",
                            params![task_id, step_id],
                        )?;
                        tx.execute(
                            "UPDATE leases
                             SET state = 'expired'
                             WHERE task_id = ?1 AND step_id = ?2 AND state = 'active'",
                            params![task_id, step_id],
                        )?;
                    }
                }
                _ => {}
            }
        }
    }

    let mut all_steps: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT step_id
             FROM step_status
             WHERE task_id = ?1
             ORDER BY step_id",
        )?;
        let rows = stmt.query_map([task_id], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    all_steps.sort_by_key(|s| step_key(s));

    for step_id in &all_steps {
        let current: String = tx.query_row(
            "SELECT status FROM step_status WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if terminal(&current) || current == "dispatched" || current == "ready" {
            continue;
        }

        let dep_total: i64 = tx.query_row(
            "SELECT COUNT(*) FROM step_dependencies
             WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if dep_total == 0 {
            tx.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
            continue;
        }

        let dep_satisfied: i64 = tx.query_row(
            "SELECT COUNT(*)
             FROM step_dependencies d
             JOIN step_status s
               ON s.task_id = d.task_id
              AND s.step_id = d.depends_on_step_id
             WHERE d.task_id = ?1
               AND d.step_id = ?2
               AND s.status = 'committed'",
            params![task_id, step_id],
            |r| r.get(0),
        )?;

        if dep_satisfied == dep_total {
            tx.execute(
                "UPDATE step_status SET status = 'ready'
                 WHERE task_id = ?1 AND step_id = ?2 AND status = 'pending'",
                params![task_id, step_id],
            )?;
        }
    }

    let mut ready: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT s.step_id
             FROM step_status s
             WHERE s.task_id = ?1
               AND s.status = 'ready'
               AND NOT EXISTS (
                   SELECT 1
                   FROM leases l
                   WHERE l.task_id = s.task_id
                     AND l.step_id = s.step_id
                     AND l.state = 'active'
               )
             ORDER BY s.step_id",
        )?;
        let rows = stmt.query_map([task_id], |r| r.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    ready.sort_by_key(|s| step_key(s));
    println!("READY_QUEUE: {:?}", ready);

    for step_id in ready {
        let current_generation: i64 = tx.query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
            [],
            |r| r.get(0),
        )?;

        let lease_seq: i64 = tx.query_row(
            "SELECT COALESCE(COUNT(*), 0) + 1 FROM leases WHERE task_id = ?1 AND step_id = ?2",
            params![task_id, step_id],
            |r| r.get(0),
        )?;
        let lease_id = format!("{task_id}/{step_id}/lease/{lease_seq}");

        let inserted = tx.execute(
            "INSERT INTO leases
             (lease_id, task_id, step_id, worker_id, acquired_generation, expires_at_generation, state)
             SELECT ?1, ?2, ?3, 'worker-scheduler', ?4, ?5, 'active'
             WHERE NOT EXISTS (
                 SELECT 1 FROM leases
                 WHERE task_id = ?2 AND step_id = ?3 AND state = 'active'
             )",
            params![lease_id, task_id, step_id, current_generation, current_generation + 2],
        )?;

        if inserted == 0 {
            continue;
        }

        tx.execute(
            "UPDATE step_status
             SET status = 'dispatched'
             WHERE task_id = ?1 AND step_id = ?2 AND status = 'ready'",
            params![task_id, step_id],
        )?;

        let causal_unit_id = current_generation + 1;

        let lease_payload = json!({
            "lease_id": lease_id,
            "worker_id": "worker-scheduler",
            "acquired_generation": current_generation,
            "expires_at_generation": current_generation + 2
        });

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 0, ?3, ?4, 'LEASE_ACQUIRED', ?5, ?6)",
            params![
                causal_unit_id,
                causal_unit_id,
                task_id,
                step_id,
                serde_json::to_string(&lease_payload)?,
                causal_unit_id
            ],
        )?;

        let dispatch_payload = json!({
            "lease_id": lease_id,
            "worker_id": "worker-scheduler"
        });

        tx.execute(
            "INSERT INTO event_log
             (system_generation, causal_unit_id, sequence_in_unit, task_id, step_id, event_type, payload, logical_generation)
             VALUES (?1, ?2, 1, ?3, ?4, 'STEP_DISPATCHED', ?5, ?6)",
            params![
                causal_unit_id,
                causal_unit_id,
                task_id,
                step_id,
                serde_json::to_string(&dispatch_payload)?,
                causal_unit_id
            ],
        )?;

        println!("DISPATCHED: {}", step_id);
    }

    tx.commit()?;

    let conn = Connection::open(db)?;
    let mut status: BTreeMap<String, String> = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT step_id, status
         FROM step_status
         WHERE task_id = ?1
         ORDER BY step_id",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        status.insert(row.0, row.1);
    }

    println!("STEP_STATUS: {:?}", status);
    Ok(())
}

pub fn current_status_map(db: &str, task_id: &str) -> Result<BTreeMap<String, String>> {
    let conn = Connection::open(db)?;
    let mut out = BTreeMap::new();

    let mut stmt = conn.prepare(
        "SELECT step_id, status
         FROM step_status
         WHERE task_id = ?1
         ORDER BY step_id",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        out.insert(row.0, row.1);
    }

    Ok(out)
}

pub fn next_ready_step(db: &str, task_id: &str) -> Result<Option<String>> {
    let conn = Connection::open(db)?;
    let step = conn
        .query_row(
            "SELECT s.step_id
             FROM step_status s
             WHERE s.task_id = ?1
               AND s.status = 'ready'
               AND NOT EXISTS (
                   SELECT 1 FROM leases l
                   WHERE l.task_id = s.task_id
                     AND l.step_id = s.step_id
                     AND l.state = 'active'
               )
             ORDER BY s.step_id
             LIMIT 1",
            [task_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok(step)
}
