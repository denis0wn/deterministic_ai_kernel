//! Unified error model for the Execution Kernel.
//!
//! All kernel-internal errors use `KernelError` instead of `anyhow::Error`.
//! This enforces structured error categorization across the kernel boundary.
//!
//! CLI-level errors use `CliError` for structured exit messages.

use std::fmt;

// ── CLI Error Type (for main.rs exit handling) ────────────────────────────────

#[derive(Debug)]
pub enum CliError {
    Database(String),
    Llm(String),
    FileSystem(String),
    Pipeline(String),
    InvalidInput(String),
    RuntimeUnavailable(String),
    Other(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Database(msg) => write!(f, "database error: {}", msg),
            CliError::Llm(msg) => write!(f, "LLM error: {}", msg),
            CliError::FileSystem(msg) => write!(f, "file system error: {}", msg),
            CliError::Pipeline(msg) => write!(f, "pipeline error: {}", msg),
            CliError::InvalidInput(msg) => write!(f, "invalid input: {}", msg),
            CliError::RuntimeUnavailable(msg) => write!(f, "runtime unavailable: {}", msg),
            CliError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CliError {}

impl From<anyhow::Error> for CliError {
    fn from(err: anyhow::Error) -> Self {
        CliError::Other(err.to_string())
    }
}

impl From<rusqlite::Error> for CliError {
    fn from(err: rusqlite::Error) -> Self {
        CliError::Database(err.to_string())
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::FileSystem(err.to_string())
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        CliError::Other(err.to_string())
    }
}

pub fn exit_with_error(error: &CliError) -> ! {
    eprintln!("Error: {}", error);
    std::process::exit(1);
}

pub fn warn(message: &str) {
    eprintln!("Warning: {}", message);
}

// ── Kernel Error Type (for internal kernel operations) ────────────────────────

#[derive(Debug)]
pub enum KernelError {
    Specification(SpecificationError),
    Validation(ValidationError),
    Execution(ExecutionError),
    Provider(ProviderError),
    Reconstruction(ReconstructionError),
}

#[derive(Debug)]
pub enum SpecificationError {
    MalformedSpec {
        detail: String,
    },
    MissingStep {
        step_id: String,
    },
    InvalidDependency {
        step_id: String,
        missing_dep: String,
    },
    UnsupportedVersion {
        version: u32,
    },
    MissingPrimitive {
        step_id: String,
    },
}

#[derive(Debug)]
pub enum ValidationError {
    HashMismatch {
        spec_id: String,
        calculated: String,
    },
    DependencyViolation {
        step_id: String,
        parent_id: String,
        detail: String,
    },
    CapabilityMismatch {
        step_id: String,
        worker_id: String,
        required: String,
        actual: String,
    },
    UnknownEventType {
        event_type: String,
    },
    InvalidTransition {
        from: String,
        to: String,
        reason: String,
    },
}

#[derive(Debug)]
pub enum ExecutionError {
    PrimitiveStartFailed { step_id: String, reason: String },
    PrimitiveTimeout { step_id: String, timeout_secs: u64 },
    LeaseExpired { step_id: String, lease_id: String },
    DoubleCommit { step_id: String },
    StaleClaim { step_id: String, worker_id: String },
}

#[derive(Debug)]
pub enum ProviderError {
    Filesystem {
        operation: String,
        path: String,
        detail: String,
    },
    Llm {
        prompt_hash: String,
        detail: String,
    },
    Persistence {
        operation: String,
        detail: String,
    },
    NotRegistered {
        provider_type: String,
    },
}

#[derive(Debug)]
pub enum ReconstructionError {
    CorruptedEventLog { task_id: String, detail: String },
    ReplayDivergence { task_id: String, detail: String },
    OrphanedEffects { task_id: String, count: u64 },
    CausalOrderViolation { unit_id: i64, detail: String },
}

// ── Display implementations ───────────────────────────────────────────────────

impl fmt::Display for KernelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Specification(e) => write!(f, "specification error: {}", e),
            Self::Validation(e) => write!(f, "validation error: {}", e),
            Self::Execution(e) => write!(f, "execution error: {}", e),
            Self::Provider(e) => write!(f, "provider error: {}", e),
            Self::Reconstruction(e) => write!(f, "reconstruction error: {}", e),
        }
    }
}

impl fmt::Display for SpecificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedSpec { detail } => write!(f, "malformed spec: {}", detail),
            Self::MissingStep { step_id } => write!(f, "missing step: {}", step_id),
            Self::InvalidDependency {
                step_id,
                missing_dep,
            } => {
                write!(
                    f,
                    "step {} depends on missing step {}",
                    step_id, missing_dep
                )
            }
            Self::UnsupportedVersion { version } => {
                write!(f, "unsupported spec version: {}", version)
            }
            Self::MissingPrimitive { step_id } => {
                write!(f, "missing primitive on step: {}", step_id)
            }
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HashMismatch {
                spec_id,
                calculated,
            } => {
                write!(
                    f,
                    "hash mismatch: spec_id={}, calculated={}",
                    spec_id, calculated
                )
            }
            Self::DependencyViolation {
                step_id,
                parent_id,
                detail,
            } => {
                write!(
                    f,
                    "dependency violation: {} -> {}: {}",
                    step_id, parent_id, detail
                )
            }
            Self::CapabilityMismatch {
                step_id,
                worker_id,
                required,
                actual,
            } => {
                write!(
                    f,
                    "capability mismatch on step {}: worker {} has {}, requires {}",
                    step_id, worker_id, actual, required
                )
            }
            Self::UnknownEventType { event_type } => {
                write!(f, "unknown event type: {}", event_type)
            }
            Self::InvalidTransition { from, to, reason } => {
                write!(f, "invalid transition {} -> {}: {}", from, to, reason)
            }
        }
    }
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PrimitiveStartFailed { step_id, reason } => {
                write!(f, "primitive start failed for {}: {}", step_id, reason)
            }
            Self::PrimitiveTimeout {
                step_id,
                timeout_secs,
            } => write!(f, "primitive {} timed out after {}s", step_id, timeout_secs),
            Self::LeaseExpired { step_id, lease_id } => {
                write!(f, "lease {} expired for step {}", lease_id, step_id)
            }
            Self::DoubleCommit { step_id } => write!(f, "double commit on step {}", step_id),
            Self::StaleClaim { step_id, worker_id } => {
                write!(f, "stale claim by {} on step {}", worker_id, step_id)
            }
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Filesystem {
                operation,
                path,
                detail,
            } => write!(f, "filesystem {}: {} — {}", operation, path, detail),
            Self::Llm {
                prompt_hash,
                detail,
            } => write!(f, "llm (prompt {}): {}", prompt_hash, detail),
            Self::Persistence { operation, detail } => {
                write!(f, "persistence {}: {}", operation, detail)
            }
            Self::NotRegistered { provider_type } => {
                write!(f, "provider not registered: {}", provider_type)
            }
        }
    }
}

impl fmt::Display for ReconstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CorruptedEventLog { task_id, detail } => {
                write!(f, "corrupted event log for task {}: {}", task_id, detail)
            }
            Self::ReplayDivergence { task_id, detail } => {
                write!(f, "replay divergence for task {}: {}", task_id, detail)
            }
            Self::OrphanedEffects { task_id, count } => {
                write!(f, "task {} has {} orphaned effects", task_id, count)
            }
            Self::CausalOrderViolation { unit_id, detail } => {
                write!(f, "causal order violation in unit {}: {}", unit_id, detail)
            }
        }
    }
}

impl std::error::Error for KernelError {}

impl From<SpecificationError> for KernelError {
    fn from(e: SpecificationError) -> Self {
        Self::Specification(e)
    }
}

impl From<ValidationError> for KernelError {
    fn from(e: ValidationError) -> Self {
        Self::Validation(e)
    }
}

impl From<ExecutionError> for KernelError {
    fn from(e: ExecutionError) -> Self {
        Self::Execution(e)
    }
}

impl From<ProviderError> for KernelError {
    fn from(e: ProviderError) -> Self {
        Self::Provider(e)
    }
}

impl From<ReconstructionError> for KernelError {
    fn from(e: ReconstructionError) -> Self {
        Self::Reconstruction(e)
    }
}

pub type KernelResult<T> = std::result::Result<T, KernelError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_error_display() {
        let err = CliError::Database("connection failed".into());
        assert_eq!(err.to_string(), "database error: connection failed");
    }

    #[test]
    fn kernel_error_display_formats_correctly() {
        let err = KernelError::Validation(ValidationError::HashMismatch {
            spec_id: "abc123".to_string(),
            calculated: "def456".to_string(),
        });
        let msg = format!("{}", err);
        assert!(msg.contains("hash mismatch"));
        assert!(msg.contains("abc123"));
    }

    #[test]
    fn kernel_error_from_specification() {
        let spec_err = SpecificationError::MissingStep {
            step_id: "step_01".to_string(),
        };
        let kernel_err: KernelError = spec_err.into();
        assert!(matches!(kernel_err, KernelError::Specification(_)));
    }

    #[test]
    fn all_error_categories_are_display() {
        let errors: Vec<KernelError> = vec![
            SpecificationError::MalformedSpec {
                detail: "bad json".into(),
            }
            .into(),
            ValidationError::DependencyViolation {
                step_id: "s1".into(),
                parent_id: "s0".into(),
                detail: "parent not completed".into(),
            }
            .into(),
            ExecutionError::DoubleCommit {
                step_id: "s1".into(),
            }
            .into(),
            ProviderError::Filesystem {
                operation: "read".into(),
                path: "/tmp/x".into(),
                detail: "not found".into(),
            }
            .into(),
            ReconstructionError::OrphanedEffects {
                task_id: "t1".into(),
                count: 3,
            }
            .into(),
        ];
        for err in &errors {
            let _ = format!("{}", err);
        }
    }
}
