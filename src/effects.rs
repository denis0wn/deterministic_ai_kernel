use anyhow::Result;
use rusqlite::{params, Connection};
use serde_json::json;

pub fn execute_effects(db: &str, task_id: &str) -> Result<()> {
    let conn = Connection::open(db)?;

    let mut stmt = conn.prepare(
        "SELECT e.effect_id, e.task_id, e.step_id, e.state
         FROM effect_ledger e
         LEFT JOIN external_effects x ON x.effect_id = e.effect_id
         WHERE e.task_id = ?1
           AND e.state IN ('committed','rejected')
           AND x.effect_id IS NULL
         ORDER BY e.effect_id",
    )?;

    let rows = stmt.query_map([task_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    let mut count = 0_i64;

    for row in rows {
        let (effect_id, task_id, step_id, observed_state) = row?;

        let result_payload = if observed_state == "committed" {
            json!({
                "effect_id": effect_id,
                "action": "send_to_external_system",
                "status": "executed"
            })
        } else {
            json!({
                "effect_id": effect_id,
                "action": "skip_external_side_effect",
                "status": "suppressed_due_to_rejection"
            })
        };

        conn.execute(
            "INSERT INTO external_effects
             (effect_id, task_id, step_id, observed_state, result_payload)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                effect_id,
                task_id,
                step_id,
                observed_state,
                serde_json::to_string(&result_payload)?
            ],
        )?;

        count += 1;
    }

    println!("EFFECT_EXECUTION_OK");
    println!("EXECUTED_EFFECT_ROWS: {}", count);
    Ok(())
}
