# Architecture — deterministic_ai_kernel

> **Version:** post-`7946397` (cleanup pass)
> **Stack:** Rust 2021 · Tokio async · SQLite (rusqlite bundled) · BLAKE3 hashing · JSON Schema validation

---

## Overview

`deterministic_ai_kernel` is a deterministic execution kernel for AI workflows. Its central guarantee: given the same seed and inputs, every plan execution produces identical, cryptographically verifiable output — regardless of environment or LLM nondeterminism.

The kernel handles workflow planning, semantic bias injection, replay, snapshot versioning, and LLM routing. All persistent state is content-addressed; all CLI surfaces are JSON-stable contracts.

---

## Module Map

```text
src/
├── main.rs                      # Binary entry point
├── bin/run.rs                   # Alternate runner binary
├── lib.rs                       # Crate root / re-exports
├── api.rs                       # HTTP API surface (reqwest-based)
├── cli_json.rs                  # JSON CLI contract layer
│
├── kernel_types.rs              # Core domain types
├── effects.rs                   # Side-effect descriptors
├── event_bus.rs                 # Intra-kernel event routing
├── scheduler.rs                 # Task scheduling
├── worker.rs                    # Worker pool
├── leases.rs                    # Lease / lock primitives
├── snapshot.rs                  # Snapshot capture & versioning
│
├── llm.rs                       # LLM client abstraction
├── lm_control/                  # LLM routing & policy
│   ├── mod.rs
│   └── policy.rs                # Routing rules: safe-switch, auto-route, blocked
│
├── model_manifest.rs            # Per-model capability manifest
├── model_registry.rs            # Runtime model registry
│
├── embeddings.rs                # Embedding utilities
│
├── execution/                   # Workflow execution runtime
│   ├── mod.rs
│   └── runtime.rs
│
├── planner_pipeline/            # Plan lifecycle: parse → normalize → execute → report
│   ├── mod.rs
│   ├── pipeline.rs              # Orchestration of pipeline stages
│   ├── parser.rs                # Raw plan parsing
│   ├── normalizer.rs            # Canonical plan form
│   ├── execution_engine.rs      # Step-level execution
│   ├── critic.rs                # Post-execution quality critique
│   ├── plan_diff.rs             # Structural plan diffing
│   ├── semantic_mapper.rs       # Step → semantic concept mapping
│   ├── persistence.rs           # Plan artifact persistence (SQLite)
│   ├── replay.rs                # Plan-level replay
│   └── report.rs                # Execution report generation
│
├── replay/                      # Deterministic replay engine
│   ├── mod.rs
│   ├── engine.rs                # Core replay loop
│   └── capsule.rs               # Replay capsule builder & serialisation
│
├── registry/
│   └── mod.rs                   # Artifact registry
│
├── schema/                      # JSON Schema validation
│   ├── mod.rs
│   └── validator.rs
│
├── semantic_bias/
│   └── mod.rs                   # Semantic bias types & application
│
└── workflow/                    # Workflow compiler, planner, contracts
    ├── mod.rs
    ├── compiler.rs              # Workflow → execution plan
    ├── planner.rs               # LLM-driven planning
    ├── contract.rs              # Workflow contract / interface types
    └── semantic/
        ├── mod.rs
        ├── bias.rs              # BiasConfiguration & seed-matrix
        └── interpreter.rs       # Bias interpretation at runtime

