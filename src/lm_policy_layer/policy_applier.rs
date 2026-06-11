#![allow(dead_code)]

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::fs;

use super::{
    collect_policy_state, latest_policy_version, new_version_id, write_policy_version,
    PolicyState, PolicyUpdate, PolicyUpdateEnvelope, PolicyVersionRecord,
};

const MIN_CONFIDENCE: f64 = 0.7;

pub fn apply_policy_update(envelope: &PolicyUpdateEnvelope) -> Result<PolicyVersionRecord> {
    if envelope.confidence < MIN_CONFIDENCE {
        return Err(anyhow!("policy confidence below minimum threshold"));
    }

    let rollback_pointer = latest_policy_version()?.map(|v| v.version_id);
    let mut snapshot = collect_policy_state()?;

    for update in &envelope.parameter_updates {
        super::ensure_surface_is_allowed(&update.surface)?;
        apply_single_update(&mut snapshot, update)?;
    }

    let record = PolicyVersionRecord {
        version_id: new_version_id()?,
        created_at_unix: unix_now()?,
        confidence: envelope.confidence,
        snapshot,
        diff: envelope.parameter_updates.clone(),
        rollback_pointer,
    };

    write_policy_version(&record)?;
    append_apply_log(&record)?;
    Ok(record)
}

fn apply_single_update(snapshot: &mut PolicyState, update: &PolicyUpdate) -> Result<()> {
    match update.surface.as_str() {
        "planner" => {
            if update.key == "fallback_keywords" {
                let arr = update
                    .new
                    .as_array()
                    .ok_or_else(|| anyhow!("planner fallback_keywords must be array"))?;
                snapshot.planner_heuristics_policy.fallback_keywords = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                return Ok(());
            }
        }
        "ram_gating" => {
            for row in &mut snapshot.ram_gating_policy.thresholds_gb {
                if row.ram_class == update.key {
                    row.threshold_gb = parse_f64_value(&update.new)?;
                    return Ok(());
                }
            }
        }
        "model_selection" => {
            for row in &mut snapshot.model_selection_policy.roles {
                if row.role == update.key {
                    row.selected_model = parse_string_value(&update.new)?;
                    return Ok(());
                }
            }
        }
        "env_sync" => {
            for row in &mut snapshot.env_sync_policy.managed_roles {
                if row.role == update.key {
                    row.env_key = parse_string_value(&update.new)?;
                    return Ok(());
                }
            }
        }
        _ => {}
    }

    Err(anyhow!(
        "unsupported policy update target: surface={} key={}",
        update.surface,
        update.key
    ))
}

fn parse_f64_value(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .ok_or_else(|| anyhow!("expected numeric policy value"))
}

fn parse_string_value(value: &Value) -> Result<String> {
    value
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("expected string policy value"))
}

fn unix_now() -> Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| anyhow!("system clock before unix epoch"))?
        .as_secs())
}

fn append_apply_log(record: &PolicyVersionRecord) -> Result<()> {
    let line = serde_json::to_string(record)?;
    let path = "policy_versions/apply.log";
    let mut content = fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&line);
    content.push('\n');
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_low_confidence_update() {
        let envelope = PolicyUpdateEnvelope {
            parameter_updates: vec![],
            confidence: 0.2,
        };
        assert!(apply_policy_update(&envelope).is_err());
    }
}
