//! Phase 2B — Solver provider abstraction.
//!
//! [`SolverProvider`] is a synchronous, dependency-injected trait that all
//! constraint-solving back-ends must implement.  The only built-in
//! implementation is [`NullSolverProvider`], which always returns
//! [`SolverError::Unsupported`].  A real SMT / Z3 back-end can be added
//! later without touching `PrimitiveExecutor`.

use serde::{Deserialize, Serialize};

// ── Public data types ─────────────────────────────────────────────────────────

/// The category of constraint problem being submitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// A raw SMT-LIB 2 query.
    Smt2,
    /// Placeholder for future LP / ILP problems.
    LinearArithmetic,
}

/// Input submitted to a [`SolverProvider`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverProblem {
    /// Unique identifier (mirrors PrimitiveSpec id).
    pub id: String,
    /// Class of problem.
    pub kind: ProblemKind,
    /// Opaque payload — for `Smt2` this is the raw SMT-LIB 2 source text.
    pub payload: String,
}

/// Outcome status returned by a solver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SolveStatus {
    Sat,
    Unsat,
    Unknown,
}

/// Output produced by a [`SolverProvider`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverResult {
    pub status: SolveStatus,
    /// Raw solver output / model (empty when `unsat` or `unknown`).
    pub solution: String,
    /// Arbitrary key-value metadata (e.g. solver name, version, wall-time).
    pub metadata: serde_json::Value,
}

/// Errors a [`SolverProvider`] may return.
#[derive(Debug)]
pub enum SolverError {
    Unsupported,
    ExecutionFailed(String),
    BadPayload(String),
}

impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolverError::Unsupported => {
                write!(
                    f,
                    "Unsupported: solver backend is not configured (NullSolverProvider)"
                )
            }
            SolverError::ExecutionFailed(msg) => write!(f, "solver execution failed: {}", msg),
            SolverError::BadPayload(msg) => write!(f, "malformed problem payload: {}", msg),
        }
    }
}

impl std::error::Error for SolverError {}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Dependency-injected interface for all constraint-solving back-ends.
///
/// Implementations must be `Send + Sync` so they can be shared across threads
/// inside `RuntimeManager`.
pub trait SolverProvider: Send + Sync {
    fn solve(&self, problem: SolverProblem) -> Result<SolverResult, SolverError>;
}

// ── Null implementation ───────────────────────────────────────────────────────

/// Temporary no-op provider.  Always returns [`SolverError::Unsupported`].
///
/// Registered by default in `RuntimeManager`.  Replace with a real back-end
/// (e.g. `Z3SolverProvider`) when SMT solving is needed.
pub struct NullSolverProvider;

impl SolverProvider for NullSolverProvider {
    fn solve(&self, _problem: SolverProblem) -> Result<SolverResult, SolverError> {
        Err(SolverError::Unsupported)
    }
}
