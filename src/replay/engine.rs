use rusqlite::{Connection, OpenFlags};
use std::collections::BTreeMap;

pub fn replay_validate(db: &str, task_id: &str) -> bool {
    if !std::path::Path::new(db).exists() {
        return true;
    }
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT causal_unit_id, sequence_in_unit, event_type
             FROM event_log
             WHERE task_id = ?1
             ORDER BY causal_unit_id, sequence_in_unit",
        )
        .unwrap();

    let rows = stmt
        .query_map([task_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap();

    let mut units: BTreeMap<i64, Vec<(i64, String)>> = BTreeMap::new();
    for row in rows.filter_map(|r| r.ok()) {
        units.entry(row.0).or_default().push((row.1, row.2));
    }

    let mut ok = true;

    for (unit_id, events) in &units {
        for (i, (seq, _)) in events.iter().enumerate() {
            if *seq != i as i64 {
                eprintln!("INVALID unit {}: gap at {}", unit_id, i);
                ok = false;
            }
        }

        let types: Vec<&str> = events.iter().map(|(_, t)| t.as_str()).collect();
        let pos = |name: &str| types.iter().position(|&t| t == name);

        if let (Some(a), Some(b)) = (pos("LEASE_ACQUIRED"), pos("EFFECT_RESERVED")) {
            if a >= b {
                eprintln!(
                    "INVALID unit {}: LEASE_ACQUIRED must precede EFFECT_RESERVED",
                    unit_id
                );
                ok = false;
            }
        }

        if let (Some(a), Some(b)) = (pos("EFFECT_RESERVED"), pos("STEP_DISPATCHED")) {
            if a >= b {
                eprintln!(
                    "INVALID unit {}: EFFECT_RESERVED must precede STEP_DISPATCHED",
                    unit_id
                );
                ok = false;
            }
        }

        let completed = pos("STEP_COMPLETED");
        let failed = pos("STEP_FAILED");

        if completed.is_some() && failed.is_some() {
            eprintln!(
                "INVALID unit {}: both STEP_COMPLETED and STEP_FAILED present",
                unit_id
            );
            ok = false;
        }

        if completed.is_none() && failed.is_none() && types.contains(&"STEP_DISPATCHED") {
            eprintln!(
                "INVALID unit {}: dispatched step has no terminal event",
                unit_id
            );
            ok = false;
        }
    }

    let reserved: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM effect_ledger WHERE task_id = ?1 AND state = 'reserved'",
            [task_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let committed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM effect_ledger WHERE task_id = ?1 AND state = 'committed'",
            [task_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let rejected: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM effect_ledger WHERE task_id = ?1 AND state = 'rejected'",
            [task_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if reserved != 0 {
        eprintln!(
            "INVALID task {}: {} reserved effects remain",
            task_id, reserved
        );
        ok = false;
    }

    println!("REPLAY {}", if ok { "VALID" } else { "INVALID" });
    println!("COMMITTED_EFFECTS: {}", committed);
    println!("REJECTED_EFFECTS: {}", rejected);
    ok
}
