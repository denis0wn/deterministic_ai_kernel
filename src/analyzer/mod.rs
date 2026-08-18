//! ANALYZER layer — built ON TOP of the frozen kernel (see ROLES.md).
//!
//! Invariants enforced across every submodule:
//! - READ-ONLY against the scanned workspace: no file writes, no patches,
//!   no test runs. Anything that mutates code goes through the EXECUTOR
//!   (deterministic_ai_kernel_clean), and only via a human operator.
//! - Deterministic: same inputs + same analyzer version ⇒ same
//!   candidates, findings, tasks, manifests, decisions, work orders and
//!   chain reports (sorting everywhere, no randomness, no clock reads in
//!   byte-stable outputs; timestamps live only in operational envelopes).
//! - Model/heuristic/external-tool output is UNTRUSTED HINT: candidates
//!   never become effects on their own; model hints are always marked
//!   `model_hint_unverified`; executor evidence is re-checked from bytes.
//!
//! v0.4-pilot-ops boundary modules:
//! - `review_gate` — human approval as a tamper-evident artifact;
//! - `work_order` — passive handoff document (executes nothing);
//! - `evidence_chain` — independent verification by recomputed hashes.

pub mod audit_log;
pub mod evidence_chain;
pub mod evidence_manifest;
pub mod external_sast;
pub mod hint_engine;
pub mod ingestion;
pub mod monetary_oracle;
pub mod operational;
pub mod pilot_package;
pub mod pilot_report;
pub mod review_gate;
pub mod scan_primitives;
pub mod task_emitter;
pub mod triage;
pub mod work_order;

pub const ANALYZER_VERSION: &str = "0.4.1";
