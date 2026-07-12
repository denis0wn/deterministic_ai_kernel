#![allow(dead_code)]
use serde_json::Value;
use std::fmt;

const SEMANTIC_BIAS_V1_SCHEMA: &str = include_str!("../../schema/semantic_bias_v1.schema.json");

#[derive(Debug)]
pub struct SchemaValidationError(pub String);

impl fmt::Display for SchemaValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "schema validation error: {}", self.0)
    }
}

impl std::error::Error for SchemaValidationError {}

pub fn validate_semantic_bias_v1(value: &Value) -> Result<(), SchemaValidationError> {
    let schema: Value = serde_json::from_str(SEMANTIC_BIAS_V1_SCHEMA)
        .expect("semantic_bias_v1.schema.json is not valid JSON");

    let compiled =
        jsonschema::validator_for(&schema).expect("semantic_bias_v1.schema.json failed to compile");

    let errors: Vec<String> = compiled.iter_errors(value).map(|e| e.to_string()).collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(SchemaValidationError(errors.join("; ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_bias() -> serde_json::Value {
        json!({
            "version": "v1",
            "seed": 42,
            "preferred": ["AnalyzeTask", "PlanExecution"],
            "weights": {
                "TightenPlannerPrompt": 1.0,
                "NormalizePlannerOutput": 1.0,
                "AddLlmFallbackHandling": 1.0,
                "AddPlannerTestCoverage": 1.0,
                "ValidatePlannerOutput": 1.0,
                "AnalyzeTask": 1.0,
                "PlanExecution": 1.0,
                "ExecuteChanges": 1.0,
                "ReadRepository": 1.0,
                "LocateBug": 1.0,
                "PatchCode": 1.0,
                "RunTests": 1.0,
                "ValidatePatch": 1.0
            }
        })
    }

    #[test]
    fn accepts_valid_bias() {
        assert!(validate_semantic_bias_v1(&valid_bias()).is_ok());
    }

    #[test]
    fn rejects_unknown_weights_key() {
        let mut v = valid_bias();
        v["weights"]["banana"] = json!(1.0);
        assert!(validate_semantic_bias_v1(&v).is_err());
    }

    #[test]
    fn rejects_unknown_preferred_value() {
        let mut v = valid_bias();
        v["preferred"] = json!(["foobar"]);
        assert!(validate_semantic_bias_v1(&v).is_err());
    }

    #[test]
    fn rejects_unknown_version() {
        let mut v = valid_bias();
        v["version"] = json!("v2");
        assert!(validate_semantic_bias_v1(&v).is_err());
    }

    #[test]
    fn rejects_missing_required_field() {
        let mut v = valid_bias();
        v.as_object_mut().expect("bias is an object").remove("seed");
        assert!(validate_semantic_bias_v1(&v).is_err());
    }

    #[test]
    fn rejects_additional_properties() {
        let mut v = valid_bias();
        v["unknown_field"] = json!("oops");
        assert!(validate_semantic_bias_v1(&v).is_err());
    }
}
