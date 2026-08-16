//! ANALYZER layer — built ON TOP of the frozen kernel (see ROLES.md).
//!
//! Invariants enforced across every submodule:
//! - READ-ONLY against the scanned workspace: no file writes, no patches,
//!   no test runs. Anything that mutates code goes through the EXECUTOR
//!   (deterministic_ai_kernel_clean) via the TaskContract in
//!   `task_emitter`.
//! - Deterministic: same workspace + same analyzer version ⇒ same
//!   candidates, findings, tasks, manifests (BTreeMap/Vec sorting
//!   everywhere, no randomness, no clock reads in byte-stable outputs;
//!   timestamps live only in the operational audit log).
//! - Model/heuristic output is UNTRUSTED HINT: candidates never become
//!   effects on their own; model hints are always marked
//!   `model_hint_unverified`.

pub mod audit_log;
pub mod evidence_manifest;
pub mod ingestion;
pub mod pilot_report;
pub mod scan_primitives;
pub mod task_emitter;
pub mod triage;

pub const ANALYZER_VERSION: &str = "0.2.0";
