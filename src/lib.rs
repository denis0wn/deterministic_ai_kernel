// ============================================================
// Layer topology (PHASE B structural migration)
//
// kernel/
//   core/       — data primitives (types, effects, snapshot)
//   invariant/  — deterministic reconstruction (replay)
//               NOTE: kernel/invariant → engine/event_bus is the
//               only documented cross-layer upward reference.
// engine/       — event_bus (execution layer, steps 4+ will add
//                 leases, scheduler, worker, workflow, execution)
// domain/       — (steps 4-5: llm, lm_control, model_*, embeddings)
// interface/    — (step 5: api, cli_json)
//
// Old flat paths are preserved as #[deprecated] compatibility
// re-exports and will be removed in the final cleanup commit.
// ============================================================

// ── New canonical module tree ────────────────────────────────
pub mod kernel;
pub mod engine;

// ── Flat modules not yet migrated (steps 4-5) ───────────────
pub mod execution;
pub mod leases;
pub mod scheduler;
pub mod workflow;
pub mod worker;

pub mod llm;
pub mod model_manifest;
pub mod model_registry;
pub mod embeddings;

pub(crate) mod lm_control;

pub mod api;
pub(crate) mod cli_json;

// ── Deprecated compatibility re-exports (remove in cleanup commit) ──
// effects: REMOVED — zero users after api.rs migration (commit-F)
// snapshot: REMOVED — zero users after api.rs migration (commit-F)

#[deprecated(
    since = "phase-2b",
    note = "Use crate::engine::event_bus instead — bridge removed in cleanup commit"
)]
pub use engine::event_bus;

#[deprecated(
    since = "phase-2b",
    note = "Use crate::kernel::invariant::replay instead — bridge removed in cleanup commit"
)]
pub use kernel::invariant::replay;

#[deprecated(
    since = "phase-2b",
    note = "Use crate::kernel::core::types instead — bridge removed in cleanup commit"
)]
pub use kernel::core::types as kernel_types;
