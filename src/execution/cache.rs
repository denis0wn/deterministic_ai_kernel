use crate::execution_abi::primitives::PrimitiveKind;
use serde_json::Value;
use std::collections::BTreeMap;

pub const CACHE_SCHEMA_VERSION: &str = "v1";

pub trait PrimitiveDeterminism {
    const CACHEABLE: bool;
    const PRIMITIVE_VERSION: &'static str = "1";
}

pub struct ComputePrimitive;
pub struct ReasoningPrimitive;
pub struct ToolExecutionPrimitive;
pub struct ReadPrimitive;
pub struct WritePrimitive;
pub struct RoutePrimitive;
pub struct WaitPrimitive;
pub struct SignalPrimitive;
pub struct SpawnPrimitive;
pub struct CompletePrimitive;
pub struct FailPrimitive;
pub struct SolveConstraintPrimitive;

impl PrimitiveDeterminism for ComputePrimitive {
    const CACHEABLE: bool = true;
}
impl PrimitiveDeterminism for ReasoningPrimitive {
    const CACHEABLE: bool = true;
}
impl PrimitiveDeterminism for ToolExecutionPrimitive {
    const CACHEABLE: bool = false;
}
impl PrimitiveDeterminism for ReadPrimitive {
    const CACHEABLE: bool = true;
}
impl PrimitiveDeterminism for WritePrimitive {
    const CACHEABLE: bool = false;
}
impl PrimitiveDeterminism for RoutePrimitive {
    const CACHEABLE: bool = true;
}
impl PrimitiveDeterminism for WaitPrimitive {
    const CACHEABLE: bool = false;
}
impl PrimitiveDeterminism for SignalPrimitive {
    const CACHEABLE: bool = false;
}
impl PrimitiveDeterminism for SpawnPrimitive {
    const CACHEABLE: bool = false;
}
impl PrimitiveDeterminism for CompletePrimitive {
    const CACHEABLE: bool = false;
}
impl PrimitiveDeterminism for FailPrimitive {
    const CACHEABLE: bool = false;
}
impl PrimitiveDeterminism for SolveConstraintPrimitive {
    const CACHEABLE: bool = false;
    const PRIMITIVE_VERSION: &'static str = "1";
}

pub fn is_cacheable(kind: PrimitiveKind) -> bool {
    match kind {
        PrimitiveKind::Compute => <ComputePrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Reasoning => <ReasoningPrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::ToolExecution => <ToolExecutionPrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Read => <ReadPrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Write => <WritePrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Route => <RoutePrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Wait => <WaitPrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Signal => <SignalPrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Spawn => <SpawnPrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Complete => <CompletePrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::Fail => <FailPrimitive as PrimitiveDeterminism>::CACHEABLE,
        PrimitiveKind::SolveConstraint => {
            <SolveConstraintPrimitive as PrimitiveDeterminism>::CACHEABLE
        }
    }
}

pub fn get_primitive_version(kind: PrimitiveKind) -> &'static str {
    match kind {
        PrimitiveKind::Compute => <ComputePrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Reasoning => <ReasoningPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::ToolExecution => {
            <ToolExecutionPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION
        }
        PrimitiveKind::Read => <ReadPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Write => <WritePrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Route => <RoutePrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Wait => <WaitPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Signal => <SignalPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Spawn => <SpawnPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Complete => <CompletePrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::Fail => <FailPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION,
        PrimitiveKind::SolveConstraint => {
            <SolveConstraintPrimitive as PrimitiveDeterminism>::PRIMITIVE_VERSION
        }
    }
}

pub fn generate_cache_key(
    primitive_type: &str,
    primitive_version: &str,
    normalized_input: &Value,
    environment_fingerprint: &str,
    dependency_hash: Option<&str>,
) -> String {
    let mut map = BTreeMap::new();
    map.insert(
        "cache_schema_version".to_string(),
        Value::String(CACHE_SCHEMA_VERSION.to_string()),
    );
    map.insert(
        "primitive_type".to_string(),
        Value::String(primitive_type.to_string()),
    );
    map.insert(
        "primitive_version".to_string(),
        Value::String(primitive_version.to_string()),
    );
    map.insert("normalized_input".to_string(), normalized_input.clone());
    map.insert(
        "environment_fingerprint".to_string(),
        Value::String(environment_fingerprint.to_string()),
    );
    if let Some(dep) = dependency_hash {
        map.insert(
            "dependency_hash".to_string(),
            Value::String(dep.to_string()),
        );
    } else {
        map.insert("dependency_hash".to_string(), Value::Null);
    }

    let serialized = serde_json::to_string(&map).unwrap_or_default();
    let hash_bytes = crate::fingerprint::sha256(serialized.as_bytes());
    hash_bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn generate_planner_cache_key(
    manifest_version: &str,
    planner_version: &str,
    environment_fingerprint: &str,
    repository_fingerprint: Option<&str>,
    normalized_prompt: &str,
) -> String {
    let mut map = BTreeMap::new();
    map.insert(
        "manifest_version".to_string(),
        Value::String(manifest_version.to_string()),
    );
    map.insert(
        "planner_version".to_string(),
        Value::String(planner_version.to_string()),
    );
    map.insert(
        "environment_fingerprint".to_string(),
        Value::String(environment_fingerprint.to_string()),
    );
    map.insert(
        "repository_fingerprint".to_string(),
        Value::String(repository_fingerprint.unwrap_or("").to_string()),
    );
    map.insert(
        "normalized_prompt".to_string(),
        Value::String(normalized_prompt.to_string()),
    );

    let serialized = serde_json::to_string(&map).unwrap_or_default();
    let hash_bytes = crate::fingerprint::sha256(serialized.as_bytes());
    hash_bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
