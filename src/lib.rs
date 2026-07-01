pub mod api;
pub(crate) mod cli_json;
pub mod effects;
pub mod event_bus;
pub mod execution;
pub mod leases;
pub mod llm;
pub mod model_registry;
pub mod replay;
pub mod scheduler;
pub mod snapshot;
pub mod workflow;

pub mod model_manifest;

pub(crate) mod lm_control;

pub mod embeddings;
pub mod kernel_types;

// ============================================================
// PHASE B structural layer — kernel/core introduced in step-1
// Old flat paths preserved as deprecated bridges until step-8
// ============================================================
pub mod kernel;

#[deprecated(note = "phase-B migration bridge (step-1): use crate::kernel::core::types — remove in step-8")]
#[allow(unused_imports)]
pub use kernel::core::types as kernel_types_new;

#[deprecated(note = "phase-B migration bridge (step-1): use crate::kernel::core::snapshot — remove in step-8")]
#[allow(unused_imports)]
pub use kernel::core::snapshot as snapshot_new;

#[deprecated(note = "phase-B migration bridge (step-1): use crate::kernel::core::effects — remove in step-8")]
#[allow(unused_imports)]
pub use kernel::core::effects as effects_new;
