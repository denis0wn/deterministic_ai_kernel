use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::cli_json::{comparison_report, command_report};
use crate::event_bus;
use crate::kernel_types::ReplayCapsule;
use crate::lm_control;
use crate::replay::capsule::build_replay_capsule;
use crate::snapshot;

pub fn integrity_json_report(db: &str) -> Value {
    use std::fs;

    if std::path::Path::new(db).exists() {
        let _ = fs::remove_file(db);
        let _ = fs::remove_file(format!("{db}-wal"));
        let _ = fs::remove_file(format!("{db}-shm"));
    }

    let conn = Connection::open(db).unwrap();
    conn.execute_batch(include_str!("../event_bus/schema.sql")).unwrap();
    drop(conn);

    snapshot::rebuild_snapshot(db, "integrity-task", true).unwrap();
    snapshot::restore_snapshot(db, "integrity-task", true).unwrap();

    let conn = Connection::open(db).unwrap();
    let payload: String = conn
        .query_row(
            "SELECT payload FROM state_snapshots WHERE task_id = ?1 ORDER BY snapshot_id DESC LIMIT 1",
            ["integrity-task"],
            |r| r.get(0),
        )
        .unwrap();

    let parsed: Value = serde_json::from_str(&payload).unwrap();

    json!({
        "ok": true,
        "snapshot_version": parsed.get("snapshot_version").and_then(|v| v.as_u64()).unwrap_or(0),
        "schema_version": parsed.get("schema_version").and_then(|v| v.as_u64()).unwrap_or(0),
        "created_at_present": parsed.get("created_at").and_then(|v| v.as_u64()).is_some(),
        "state_hash_present": parsed.get("state_hash").and_then(|v| v.as_u64()).is_some(),
        "state_present": parsed.get("state").and_then(|v| v.as_object()).is_some(),
        "task_id": parsed.get("task_id").cloned().unwrap_or(Value::Null)
    })
}

pub fn integrity_json_envelope(db: &str) -> Value {
    command_report("integrity-json", integrity_json_report(db))
}

pub fn doctor_json_envelope() -> Result<Value> {
    let report = lm_control::doctor_json_report()?;
    Ok(command_report("doctor-json", report))
}

pub fn capture_capsule_save_json_envelope(db: &str, task_id: &str) -> Result<Value> {
    let bus = event_bus::EventBus::new(db).unwrap();
    let capsule = build_replay_capsule(&bus, task_id)?;
    capsule
        .validate()
        .map_err(|e| anyhow!("capture-capsule-save invalid capsule: {}", e))?;
    bus.save_replay_capsule(&capsule).unwrap();
    let report = serde_json::json!({
        "task_id": task_id,
        "execution_id": capsule.execution_id,
        "capsule_id": capsule.capsule_id,
        "valid": true,
        "events": capsule.event_ids.len(),
        "nodes": capsule.state_graph.nodes.len(),
        "edges": capsule.state_graph.edges.len(),
    });
    Ok(command_report("capture-capsule-save", report))
}

pub fn replay_capsule_json_envelope(db: &str, task_id: &str) -> Result<Value> {
    let bus = event_bus::EventBus::new(db).unwrap();
    let capsule = bus
        .latest_replay_capsule(task_id)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", task_id))?;
    let valid = capsule.validate().is_ok();
    let report = serde_json::json!({
        "task_id": task_id,
        "execution_id": capsule.execution_id,
        "capsule_id": capsule.capsule_id,
        "valid": valid,
        "events": capsule.event_ids.len(),
        "nodes": capsule.state_graph.nodes.len(),
        "edges": capsule.state_graph.edges.len(),
    });
    Ok(command_report("replay-capsule", report))
}

pub fn compare_capsules_json_envelope(db: &str, left: &str, right: &str) -> Result<Value> {
    let bus = event_bus::EventBus::new(db).unwrap();
    let left_capsule = bus
        .latest_replay_capsule(left)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", left))?;
    let right_capsule = bus
        .latest_replay_capsule(right)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", right))?;

    let report = build_compare_report(&left_capsule, &right_capsule, left, right);
    Ok(command_report("compare-capsules", report))
}

fn build_compare_report(
    left_capsule: &ReplayCapsule,
    right_capsule: &ReplayCapsule,
    left: &str,
    right: &str,
) -> Value {
    let left_valid = left_capsule.validate();
    let right_valid = right_capsule.validate();
    let left_valid_bool = left_valid.is_ok();
    let right_valid_bool = right_valid.is_ok();

    let status = if left_valid.is_err() || right_valid.is_err() {
        "structurally_invalid"
    } else if left_capsule.event_ids == right_capsule.event_ids
        && left_capsule.state_graph.nodes == right_capsule.state_graph.nodes
        && left_capsule.state_graph.edges == right_capsule.state_graph.edges
    {
        "identical"
    } else {
        "divergent"
    };

    let explanation = match status {
        "structurally_invalid" => {
            let mut reasons = Vec::new();
            if let Err(err) = &left_valid {
                reasons.push(format!("left invalid: {}", err));
            }
            if let Err(err) = &right_valid {
                reasons.push(format!("right invalid: {}", err));
            }
            reasons.join("; ")
        }
        "identical" => "event_ids, nodes, and edges match".to_string(),
        "divergent" => {
            let mut reasons = Vec::new();
            if left_capsule.event_ids != right_capsule.event_ids {
                reasons.push(format!(
                    "event_ids differ (left={}, right={})",
                    left_capsule.event_ids.len(),
                    right_capsule.event_ids.len()
                ));
            }
            if left_capsule.state_graph.nodes != right_capsule.state_graph.nodes {
                reasons.push(format!(
                    "nodes differ (left={}, right={})",
                    left_capsule.state_graph.nodes.len(),
                    right_capsule.state_graph.nodes.len()
                ));
            }
            if left_capsule.state_graph.edges != right_capsule.state_graph.edges {
                reasons.push(format!(
                    "edges differ (left={}, right={})",
                    left_capsule.state_graph.edges.len(),
                    right_capsule.state_graph.edges.len()
                ));
            }
            if reasons.is_empty() {
                "capsules differ".to_string()
            } else {
                reasons.join("; ")
            }
        }
        _ => "unknown comparison state".to_string(),
    };

    let left_only_events: Vec<_> = left_capsule
        .event_ids
        .iter()
        .filter(|id| !right_capsule.event_ids.contains(id))
        .cloned()
        .collect();
    let right_only_events: Vec<_> = right_capsule
        .event_ids
        .iter()
        .filter(|id| !left_capsule.event_ids.contains(id))
        .cloned()
        .collect();

    let left_node_pairs: Vec<(String, serde_json::Value)> = left_capsule
        .state_graph
        .nodes
        .iter()
        .map(|n| {
            let v = serde_json::to_value(n).unwrap();
            let key = serde_json::to_string(&v).unwrap();
            (key, v)
        })
        .collect();
    let left_node_keys: std::collections::HashSet<_> =
        left_node_pairs.iter().map(|(key, _)| key.clone()).collect();
    let right_node_pairs: Vec<(String, serde_json::Value)> = right_capsule
        .state_graph
        .nodes
        .iter()
        .map(|n| {
            let v = serde_json::to_value(n).unwrap();
            let key = serde_json::to_string(&v).unwrap();
            (key, v)
        })
        .collect();
    let right_node_keys: std::collections::HashSet<_> =
        right_node_pairs.iter().map(|(key, _)| key.clone()).collect();

    let left_only_nodes: Vec<_> = left_node_pairs
        .iter()
        .filter(|(key, _)| !right_node_keys.contains(key))
        .map(|(_, value)| value.clone())
        .collect();
    let right_only_nodes: Vec<_> = right_node_pairs
        .iter()
        .filter(|(key, _)| !left_node_keys.contains(key))
        .map(|(_, value)| value.clone())
        .collect();

    let left_edge_pairs: Vec<(String, serde_json::Value)> = left_capsule
        .state_graph
        .edges
        .iter()
        .map(|e| {
            let v = serde_json::to_value(e).unwrap();
            let key = serde_json::to_string(&v).unwrap();
            (key, v)
        })
        .collect();
    let left_edge_keys: std::collections::HashSet<_> =
        left_edge_pairs.iter().map(|(key, _)| key.clone()).collect();
    let right_edge_pairs: Vec<(String, serde_json::Value)> = right_capsule
        .state_graph
        .edges
        .iter()
        .map(|e| {
            let v = serde_json::to_value(e).unwrap();
            let key = serde_json::to_string(&v).unwrap();
            (key, v)
        })
        .collect();
    let right_edge_keys: std::collections::HashSet<_> =
        right_edge_pairs.iter().map(|(key, _)| key.clone()).collect();

    let left_only_edges: Vec<_> = left_edge_pairs
        .iter()
        .filter(|(key, _)| !right_edge_keys.contains(key))
        .map(|(_, value)| value.clone())
        .collect();
    let right_only_edges: Vec<_> = right_edge_pairs
        .iter()
        .filter(|(key, _)| !left_edge_keys.contains(key))
        .map(|(_, value)| value.clone())
        .collect();

    comparison_report(crate::cli_json::ComparisonReportInput {
        left_task_id: left,
        right_task_id: right,
        left_capsule,
        right_capsule,
        left_valid: left_valid_bool,
        right_valid: right_valid_bool,
        status,
        explanation: explanation.as_str(),
        left_only_events,
        right_only_events,
        left_only_nodes,
        right_only_nodes,
        left_only_edges,
        right_only_edges,
    })
}
