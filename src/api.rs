use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::cli_json::comparison_report;
use crate::event_bus;
use crate::kernel_types::ReplayCapsule;
use crate::lm_control;
use crate::replay::capsule::build_replay_capsule;
use crate::snapshot;

pub fn integrity_json_report(db: &str) -> Result<Value> {
    use std::fs;

    // The integrity check exercises the snapshot/restore machinery against a
    // disposable scratch database. The user's database is NEVER modified or
    // deleted by this read-sounding command (audit findings H3/S5: the
    // previous implementation deleted the target DB file).
    let _ = db; // signature kept for CLI compatibility; scratch DB is used
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let scratch = std::env::temp_dir().join(format!("dak_integrity_{}.db", nanos));
    let scratch_str = scratch
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF8 temp path"))?
        .to_string();

    let result = (|| -> Result<Value> {
        let conn = crate::providers::storage::open_initialized(&scratch_str)?;
        drop(conn);

        snapshot::rebuild_snapshot(&scratch_str, "integrity-task", true)?;
        snapshot::restore_snapshot(&scratch_str, "integrity-task", true)?;

        let conn = Connection::open(&scratch_str)?;
        let payload: String = conn
            .query_row(
                "SELECT payload FROM state_snapshots WHERE task_id = ?1 ORDER BY snapshot_id DESC LIMIT 1",
                ["integrity-task"],
                |r| r.get(0),
            )?;

        let parsed: Value = serde_json::from_str(&payload)?;

        Ok(json!({
            "ok": true,
            "snapshot_version": parsed.get("snapshot_version").and_then(|v| v.as_u64()).unwrap_or(0),
            "schema_version": parsed.get("schema_version").and_then(|v| v.as_u64()).unwrap_or(0),
            "created_at_present": parsed.get("created_at").and_then(|v| v.as_u64()).is_some(),
            "state_hash_present": parsed.get("state_hash").and_then(|v| v.as_u64()).is_some(),
            "state_present": parsed.get("state").and_then(|v| v.as_object()).is_some(),
            "task_id": parsed.get("task_id").cloned().unwrap_or(Value::Null)
        }))
    })();

    let _ = fs::remove_file(&scratch);
    let _ = fs::remove_file(format!("{}-wal", scratch_str));
    let _ = fs::remove_file(format!("{}-shm", scratch_str));
    result
}

pub fn doctor_json() -> Result<Value> {
    lm_control::doctor_json_report()
}

pub fn capture_capsule_save_json(db: &str, task_id: &str) -> Result<Value> {
    let report = capture_capsule_save_text(db, task_id)?;
    Ok(serde_json::json!({
        "task_id": task_id,
        "execution_id": report.execution_id,
        "capsule_id": report.capsule_id,
        "valid": true,
        "events": report.events,
        "nodes": report.nodes,
        "edges": report.edges,
    }))
}

pub struct ReplayCapsuleSummary {
    pub execution_id: String,
    pub capsule_id: String,
    pub events: usize,
    pub nodes: usize,
    pub edges: usize,
    pub valid: bool,
}

pub struct CaptureCapsuleSaveSummary {
    pub execution_id: String,
    pub capsule_id: String,
    pub events: usize,
    pub nodes: usize,
    pub edges: usize,
}

pub struct CompareCapsulesSummary {
    pub left_capsule_id: String,
    pub right_capsule_id: String,
    pub status: String,
    pub explanation: String,
}

pub fn capture_capsule_save_text(db: &str, task_id: &str) -> Result<CaptureCapsuleSaveSummary> {
    let bus = event_bus::EventBus::new(db)?;
    let capsule = build_replay_capsule(&bus, task_id)?;
    capsule
        .validate()
        .map_err(|e| anyhow!("capture-capsule-save invalid capsule: {}", e))?;
    bus.save_replay_capsule(&capsule)?;
    Ok(CaptureCapsuleSaveSummary {
        execution_id: capsule.execution_id.clone(),
        capsule_id: capsule.capsule_id.clone(),
        events: capsule.event_ids.len(),
        nodes: capsule.state_graph.nodes.len(),
        edges: capsule.state_graph.edges.len(),
    })
}

pub fn replay_capsule_text(db: &str, task_id: &str) -> Result<ReplayCapsuleSummary> {
    let bus = event_bus::EventBus::new(db)?;
    let capsule = bus
        .latest_replay_capsule(task_id)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", task_id))?;
    Ok(ReplayCapsuleSummary {
        execution_id: capsule.execution_id.clone(),
        capsule_id: capsule.capsule_id.clone(),
        events: capsule.event_ids.len(),
        nodes: capsule.state_graph.nodes.len(),
        edges: capsule.state_graph.edges.len(),
        valid: capsule.validate().is_ok(),
    })
}

pub fn replay_capsule_json(db: &str, task_id: &str) -> Result<Value> {
    let report = replay_capsule_text(db, task_id)?;
    Ok(serde_json::json!({
        "task_id": task_id,
        "execution_id": report.execution_id,
        "capsule_id": report.capsule_id,
        "valid": report.valid,
        "events": report.events,
        "nodes": report.nodes,
        "edges": report.edges,
    }))
}

pub fn compare_capsules_text(db: &str, left: &str, right: &str) -> Result<CompareCapsulesSummary> {
    let bus = event_bus::EventBus::new(db)?;
    let left_capsule = bus
        .latest_replay_capsule(left)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", left))?;
    let right_capsule = bus
        .latest_replay_capsule(right)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", right))?;

    let left_valid = left_capsule.validate();
    let right_valid = right_capsule.validate();

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

    Ok(CompareCapsulesSummary {
        left_capsule_id: left_capsule.capsule_id.clone(),
        right_capsule_id: right_capsule.capsule_id.clone(),
        status: status.to_string(),
        explanation,
    })
}

pub fn compare_capsules_json(db: &str, left: &str, right: &str) -> Result<Value> {
    let bus = event_bus::EventBus::new(db)?;
    let left_capsule = bus
        .latest_replay_capsule(left)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", left))?;
    let right_capsule = bus
        .latest_replay_capsule(right)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", right))?;

    build_compare_report(&left_capsule, &right_capsule, left, right)
}

fn build_compare_report(
    left_capsule: &ReplayCapsule,
    right_capsule: &ReplayCapsule,
    left: &str,
    right: &str,
) -> Result<Value> {
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
            let v = serde_json::to_value(n)?;
            let key = serde_json::to_string(&v)?;
            Ok((key, v))
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    let left_node_keys: std::collections::HashSet<_> =
        left_node_pairs.iter().map(|(key, _)| key.clone()).collect();
    let right_node_pairs: Vec<(String, serde_json::Value)> = right_capsule
        .state_graph
        .nodes
        .iter()
        .map(|n| {
            let v = serde_json::to_value(n)?;
            let key = serde_json::to_string(&v)?;
            Ok((key, v))
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    let right_node_keys: std::collections::HashSet<_> = right_node_pairs
        .iter()
        .map(|(key, _)| key.clone())
        .collect();

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
            let v = serde_json::to_value(e)?;
            let key = serde_json::to_string(&v)?;
            Ok((key, v))
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    let left_edge_keys: std::collections::HashSet<_> =
        left_edge_pairs.iter().map(|(key, _)| key.clone()).collect();
    let right_edge_pairs: Vec<(String, serde_json::Value)> = right_capsule
        .state_graph
        .edges
        .iter()
        .map(|e| {
            let v = serde_json::to_value(e)?;
            let key = serde_json::to_string(&v)?;
            Ok((key, v))
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    let right_edge_keys: std::collections::HashSet<_> = right_edge_pairs
        .iter()
        .map(|(key, _)| key.clone())
        .collect();

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

    Ok(comparison_report(crate::cli_json::ComparisonReportInput {
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
    }))
}
