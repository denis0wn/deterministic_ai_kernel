//! Unified error model for the Execution Kernel.
//!
//! All kernel-internal errors use `KernelError` instead of `anyhow::Error`.
//! This enforces structured error categorization across the kernel boundary.

use std::fmt;

/// Top-level error type for the Execution Kernel.
///
/// Each variant corresponds to a distinct failure domain,
/// enabling structured error handling and deterministic diagnostics.
#[derive(Debug)]
pub enum KernelError {
    /// The ExecSpec or PrimitiveSpec is malformed, missing fields, or structurally invalid.
    Specification(SpecificationError),

    /// Invariant validation failed (hash mismatch, dependency violation, capability constraint).
    Validation(ValidationError),

    /// Runtime execution of a primitive failed.
    Execution(ExecutionError),

    /// A provider (filesystem, LLM, etc.) returned an error.
    Provider(ProviderError),

    /// The primitive or runtime state was invalid for the requested operation.
    InvalidState { detail: String },

    /// Reconstruction from event log detected inconsistency.
    Reconstruction(ReconstructionError),
}

// ── Specification Errors ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum SpecificationError {
    /// ExecSpec JSON could not be parsed.
    MalformedSpec { detail: String },

    /// A referenced step_id does not exist in the spec.
    MissingStep { step_id: String },

    /// A dependency references a step that does not exist.
    InvalidDependency {
        step_id: String,
        missing_dep: String,
    },

    /// Spec version is unsupported.
    UnsupportedVersion { version: u32 },

    /// PrimitiveSpec is missing on a step that requires it.
    MissingPrimitive { step_id: String },
}

// ── Validation Errors ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ValidationError {
    /// spec_id does not match calculated hash.
    HashMismatch { spec_id: String, calculated: String },

    /// A dependency ordering violation was detected.
    DependencyViolation {
        step_id: String,
        parent_id: String,
        detail: String,
    },

    /// Worker capability does not match step requirement.
    CapabilityMismatch {
        step_id: String,
        worker_id: String,
        required: String,
        actual: String,
    },

    /// An event type is not recognized.
    UnknownEventType { event_type: String },

    /// A transition rule is invalid.
    InvalidTransition {
        from: String,
        to: String,
        reason: String,
    },
}

// ── Execution Errors ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ExecutionError {
    /// The primitive could not be started.
    PrimitiveStartFailed { step_id: String, reason: String },

    /// The primitive timed out.
    PrimitiveTimeout { step_id: String, timeout_secs: u64 },

    /// The lease expired before completion.
    LeaseExpired { step_id: String, lease_id: String },

    /// Double-commit was attempted.
    DoubleCommit { step_id: String },

    /// Worker was rejected (stale claim).
    StaleClaim { step_id: String, worker_id: String },
}

// ── Provider Errors ───────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ProviderError {
    /// Filesystem provider error.
    Filesystem {
        operation: String,
        path: String,
        detail: String,
    },

    /// LLM provider error.
    Llm { prompt_hash: String, detail: String },

    /// Database / persistence error.
    Persistence { operation: String, detail: String },

    /// Provider not registered.
    NotRegistered { provider_type: String },
}

// ── Reconstruction Errors ─────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ReconstructionError {
    /// Event log is corrupted or has gaps.
    CorruptedEventLog { task_id: String, detail: String },

    /// Replay produced different state than original execution.
    ReplayDivergence { task_id: String, detail: String },

    /// Effect ledger has orphaned reservations.
    OrphanedEffects { task_id: String, count: u64 },

    /// Event ordering violation in causal unit.
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
            Self::InvalidState { detail } => write!(f, "invalid state: {}", detail),
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
            } => {
                write!(f, "primitive {} timed out after {}s", step_id, timeout_secs)
            }
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
            } => {
                write!(f, "filesystem {}: {} — {}", operation, path, detail)
            }
            Self::Llm {
                prompt_hash,
                detail,
            } => {
                write!(f, "llm (prompt {}): {}", prompt_hash, detail)
            }
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

// ── Convenience conversions ───────────────────────────────────────────────────

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

/// Type alias for kernel operations.
pub type KernelResult<T> = std::result::Result<T, KernelError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_error_display_formats_correctly() {
        let err = KernelError::Validation(ValidationError::HashMismatch {
            spec_id: "abc123".to_string(),
            calculated: "def456".to_string(),
        });
        let msg = format!("{}", err);
        assert!(msg.contains("hash mismatch"));
        assert!(msg.contains("abc123"));
        assert!(msg.contains("def456"));
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
    fn kernel_error_from_provider() {
        let prov_err = ProviderError::NotRegistered {
            provider_type: "LLM".to_string(),
        };
        let kernel_err: KernelError = prov_err.into();
        let msg = format!("{}", kernel_err);
        assert!(msg.contains("provider not registered"));
    }

    #[test]
    fn all_error_categories_are_display() {
        // Verify all categories implement Display without panic
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
