// Reference extract of src/lm_policy_layer/lm_studio_client.rs
// This file is a deliverable snapshot for review only.
use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{PolicyContext, PolicyUpdateEnvelope};

pub fn send_policy_context(context: &PolicyContext) -> Result<String> {
    Ok(serde_json::to_string_pretty(context)?)
}

pub fn receive_policy_update(raw: &str) -> Result<PolicyUpdateEnvelope> {
    Ok(serde_json::from_str(raw)?)
}

pub fn validate_policy_output(raw: &str, min_confidence: f64) -> Result<PolicyUpdateEnvelope> {
    let envelope: PolicyUpdateEnvelope = serde_json::from_str(raw)?;
    if !(0.0..=1.0).contains(&envelope.confidence) {
        return Err(anyhow!("confidence must be between 0.0 and 1.0"));
    }
    if envelope.confidence < min_confidence {
        return Err(anyhow!(
            "confidence {:.3} below threshold {:.3}",
            envelope.confidence,
            min_confidence
        ));
    }
    for update in &envelope.parameter_updates {
        super::ensure_surface_is_allowed(&update.surface)?;
        if update.key.trim().is_empty() {
            return Err(anyhow!("policy update key must not be empty"));
        }
        if update.reason.trim().is_empty() {
            return Err(anyhow!("policy update reason must not be empty"));
        }
    }
    Ok(envelope)
}

pub fn normalize_value(value: &Value) -> Value {
    value.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_low_confidence() {
        let raw = r#"{"parameter_updates":[],"confidence":0.4}"#;
        assert!(validate_policy_output(raw, 0.7).is_err());
    }

    #[test]
    fn rejects_unknown_surface() {
        let raw = r#"{"parameter_updates":[{"surface":"execution_core","key":"x","old":1,"new":2,"reason":"bad"}],"confidence":0.9}"#;
        assert!(validate_policy_output(raw, 0.7).is_err());
    }
}
