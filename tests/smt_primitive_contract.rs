//! Phase 2B — SolveConstraint primitive dispatch contract.
//!
//! Verifies that `PrimitiveExecutor` routes `SolveConstraint` primitives to
//! the injected `SolverProvider` without touching any other primitive path.

use deterministic_ai_kernel::execution::primitive_executor::PrimitiveExecutor;
use deterministic_ai_kernel::execution::solver::{
    ProblemKind, SolveStatus, SolverError, SolverProblem, SolverProvider, SolverResult,
};
use deterministic_ai_kernel::execution_abi::primitives::{
    PrimitiveId, PrimitiveKind, PrimitiveSpec,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

// ── Recording stub ────────────────────────────────────────────────────────────

/// Records every call made to it so tests can assert dispatch occurred.
struct RecordingSolverProvider {
    calls: Mutex<Vec<SolverProblem>>,
    result: Result<SolverResult, SolverError>,
}

impl RecordingSolverProvider {
    fn returning_sat(solution: &str) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(vec![]),
            result: Ok(SolverResult {
                status: SolveStatus::Sat,
                solution: solution.to_string(),
                metadata: json!({ "provider": "RecordingSolverProvider" }),
            }),
        })
    }

    fn returning_unsupported() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(vec![]),
            result: Err(SolverError::Unsupported),
        })
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn last_call(&self) -> Option<SolverProblem> {
        self.calls.lock().unwrap().last().cloned()
    }
}

impl SolverProvider for RecordingSolverProvider {
    fn solve(&self, problem: SolverProblem) -> Result<SolverResult, SolverError> {
        self.calls.lock().unwrap().push(problem);
        // We can't clone the result, so we reconstruct:
        match &self.result {
            Ok(r) => Ok(SolverResult {
                status: r.status.clone(),
                solution: r.solution.clone(),
                metadata: r.metadata.clone(),
            }),
            Err(SolverError::Unsupported) => Err(SolverError::Unsupported),
            Err(SolverError::ExecutionFailed(s)) => Err(SolverError::ExecutionFailed(s.clone())),
            Err(SolverError::BadPayload(s)) => Err(SolverError::BadPayload(s.clone())),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_solve_spec(payload_str: &str) -> PrimitiveSpec {
    PrimitiveSpec {
        id: PrimitiveId("test-solve-1".to_string()),
        kind: PrimitiveKind::SolveConstraint,
        payload: json!({ "kind": "smt2", "payload": payload_str }),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// PrimitiveExecutor must dispatch SolveConstraint to the injected solver.
#[test]
fn test_solver_provider_dispatch() {
    let provider = RecordingSolverProvider::returning_sat("model: x = 42");
    let executor = PrimitiveExecutor::new(provider.clone() as Arc<dyn SolverProvider>);

    let spec = make_solve_spec("(declare-const x Int)(assert (= x 42))(check-sat)(get-model)");
    let result = executor
        .run("task-dispatch-test", &spec, "")
        .expect("run failed");

    // Provider was invoked exactly once.
    assert_eq!(
        provider.call_count(),
        1,
        "solver must be called exactly once"
    );

    // The forwarded problem payload must match.
    let call = provider.last_call().unwrap();
    assert_eq!(call.kind, ProblemKind::Smt2);
    assert!(call.payload.contains("check-sat"));

    // Executor output reflects solver result.
    let status = result.output.get("status").unwrap();
    assert_eq!(status, &json!(SolveStatus::Sat));
    let solution = result.output.get("solution").unwrap().as_str().unwrap();
    assert!(
        solution.contains("42"),
        "solution should contain model output"
    );
}

/// NullSolverProvider must reject SolveConstraint with an error (not panic).
#[test]
fn test_null_solver_returns_error() {
    let executor = PrimitiveExecutor::with_null_solver();
    let spec = make_solve_spec("(check-sat)");
    let result = executor.run("task-null-test", &spec, "");
    assert!(result.is_err(), "NullSolverProvider must return Err");
    let msg = format!("{:?}", result.unwrap_err());
    assert!(
        msg.contains("Unsupported") || msg.contains("SolveConstraint"),
        "error should mention Unsupported: {}",
        msg
    );
}

/// Static PrimitiveExecutor::execute must also return an error (uses Null under the hood).
#[test]
fn test_static_execute_returns_null_solver_error() {
    let spec = make_solve_spec("(check-sat)");
    let result = PrimitiveExecutor::execute("task-static-test", &spec, "");
    assert!(
        result.is_err(),
        "static execute must propagate NullSolverProvider error"
    );
}
