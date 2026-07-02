use jsonschema::JSONSchema;
use serde_json::Value;
use std::fmt;

const SEMANTIC_BIAS_V1_SCHEMA: &str =
    include_str!("../../schema/semantic_bias_v1.schema.json");

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

    let compiled = JSONSchema::compile(&schema)
        .expect("semantic_bias_v1.schema.json failed to compile");

    let result = compiled.validate(value);
    if let Err(errors) = result {
        let messages: Vec<String> = errors.map(|e| e.to_string()).collect();
        return Err(SchemaValidationError(messages.join("; ")));
    }

    Ok(())
}
