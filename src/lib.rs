pub mod api;
pub mod cli_json;
pub mod effects;
pub mod event_bus;
pub mod exec_spec;
pub mod execution;
pub mod execution_abi;
pub mod execution_identity;
pub mod grounding;
pub mod kernel_error;
pub mod leases;
pub mod llm;
pub mod model_registry;
pub mod progress;
pub mod providers;
pub mod rag;
pub mod reconstruction;
pub mod replay;
pub mod scheduler;
pub mod schema;
pub mod snapshot;
pub mod strategy;
pub mod workflow;

pub mod worker;

pub mod model_manifest;

pub mod lm_control;

pub mod mlx_lifecycle;

pub mod embeddings;
pub mod kernel_types;
pub mod planner_pipeline;
pub mod registry;
pub mod semantic_bias;
pub mod tools;

// ANALYZER layer (ROLES.md): read-only detector built ON TOP of the frozen
// kernel. This `pub mod analyzer;` declaration is the ONLY extension point
// into the kernel crate — all analyzer logic lives in src/analyzer/* and
// performs no effects (patches/tests go through the executor separately).
pub mod analyzer;
