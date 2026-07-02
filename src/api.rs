use anyhow::{anyhow, Result};
use rusqlite::Connection;
use serde_json::{json, Value};

use crate::cli_json::comparison_report;
use crate::engine::event_bus;
use crate::kernel::core::snapshot;
use crate::kernel::core::types::ReplayCapsule;
use crate::lm_control;
use crate::replay::capsule::build_replay_capsule;

// ── CLI formatting facade (delegates to internal cli_json) ───────────────────

/// Wrap a report in the standard CLI JSON envelope and print it.
pub fn emit_json(command: &str, report: Value) {
    crate::cli_json::emit_json(command, report);
}

/// Build the standard CLI JSON envelope without printing.
pub fn command_report(command: &str, report: Value) -> Value {
    crate::cli_json::command_report(command, report)
}

// ── lm_control facade ────────────────────────────────────────────────────────

/// Public mirror of lm_control::policy::SwitchPlan.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchPlan {
    pub model: String,
    pub ram_class: String,
    pub threshold_gb: f64,
    pub free_gb: f64,
}

/// Public mirror of lm_control::DoctorRoleReport.
#[derive(Debug, Clone)]
pub struct DoctorRoleReport {
    pub role: String,
    pub manifest_model: String,
    pub env_model: String,
    pub in_sync: bool,
    pub model_available: bool,
    pub switch_ready: bool,
    pub threshold_gb: f64,
}

/// Public mirror of lm_control::DoctorReport.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub free_gb: f64,
    pub lm_studio_models: usize,
    pub roles: Vec<DoctorRoleReport>,
}

/// Compute which model would be selected for a role given free_gb.
pub fn switch_plan(role: &str, free_gb: f64) -> Result<SwitchPlan> {
    let inner = lm_control::policy::switch_plan(role, free_gb)?;
    Ok(SwitchPlan {
        model: inner.model,
        ram_class: inner.ram_class,
        threshold_gb: inner.threshold_gb,
        free_gb: inner.free_gb,
    })
}

/// List available models (mock or live LM Studio).
pub fn list_models() -> Result<Vec<String>> {
    lm_control::list_models()
}

/// Full doctor report with structured fields.
pub fn doctor() -> Result<DoctorReport> {
    let inner = lm_control::doctor()?;
    Ok(DoctorReport {
        free_gb: inner.free_gb,
        lm_studio_models: inner.lm_studio_models,
        roles: inner
            .roles
            .into_iter()
            .map(|r| DoctorRoleReport {
                role: r.role,
                manifest_model: r.manifest_model,
                env_model: r.env_model,
                in_sync: r.in_sync,
                model_available: r.model_available,
                switch_ready: r.switch_ready,
                threshold_gb: r.threshold_gb,
            })
            .collect(),
    })
}

/// Print free memory stats to stdout.
pub fn print_memory(threshold_gb: Option<f64>) -> Result<()> {
    lm_control::print_memory(threshold_gb)
}

/// Print doctor report (text) to stdout.
pub fn print_doctor_text() -> Result<()> {
    lm_control::print_doctor_text()
}

/// Auto-route: verify role readiness and sync env.
pub fn auto_route(role: &str) -> Result<()> {
    lm_control::auto_route(role)
}

/// Switch model for role, with memory and availability checks.
pub fn safe_switch(role: &str) -> Result<()> {
    lm_control::safe_switch(role)
}

/// Dry-run switch: print what would happen without writing env.
pub fn dry_run_switch(role: &str) -> Result<()> {
    lm_control::dry_run_switch(role)
}

/// Doctor report as raw JSON Value (no CLI envelope).
pub fn doctor_json() -> Result<Value> {
    lm_control::doctor_json_report()
}

// ── Existing public API ──────────────────────────────────────────────────────

pub fn integrity_json_report(db: &str) -> Value {
    use std::fs;

    if std::path::Path::new(db).exists() {
        let _ = fs::remove_file(db);
        let _ = fs::remove_file(format!("{db}-wal"));
        let _ = fs::remove_file(format!("{db}-shm"));
    }

    let conn = Connection::open(db).unwrap();
    conn.execute_batch(include_str!("../event_bus/schema.sql"))
        .unwrap();
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
    let bus = event_bus::EventBus::new(db).unwrap();
    let capsule = build_replay_capsule(&bus, task_id)?;
    capsule
        .validate()
        .map_err(|e| anyhow!("capture-capsule-save invalid capsule: {}", e))?;
    bus.save_replay_capsule(&capsule).unwrap();
    Ok(CaptureCapsuleSaveSummary {
        execution_id: capsule.execution_id.clone(),
        capsule_id: capsule.capsule_id.clone(),
        events: capsule.event_ids.len(),
        nodes: capsule.state_graph.nodes.len(),
        edges: capsule.state_graph.edges.len(),
    })
}

pub fn replay_capsule_text(db: &str, task_id: &str) -> Result<ReplayCapsuleSummary> {
    let bus = event_bus::EventBus::new(db).unwrap();
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
    let bus = event_bus::EventBus::new(db).unwrap();
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
    let bus = event_bus::EventBus::new(db).unwrap();
    let left_capsule = bus
        .latest_replay_capsule(left)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", left))?;
    let right_capsule = bus
        .latest_replay_capsule(right)?
        .ok_or_else(|| anyhow!("no replay capsule found for task_id={}", right))?;

    Ok(build_compare_report(
        &left_capsule,
        &right_capsule,
        left,
        right,
    ))
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
