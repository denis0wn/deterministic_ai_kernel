# Project Roadmap: deterministic_ai_kernel Release State

This document is the single source of truth for the release readiness and evolution of the deterministic AI kernel.

## Release Invariants

All tasks must be verified with tests and code execution. General claims of readiness are invalid without passing tests.

## Phase 1: Performance Audit and Benchmark

| Task ID | Priority | Component | Description | Proof Required | Status |
|---|---|---|---|---|---|
| 1.1 | HIGH | Benchmark | Create scientifically correct KIE LLM vs LLM + Kernel comparative benchmark `tests/benchmark_llm_vs_kernel.rs` (real execution, no simulations) | Test compiles and runs successfully | [x] |
| 1.2 | HIGH | Benchmark | Implement real measurement of comparative metrics (success rate, real LLM call counts, real latency via Instant, tool executions, recovery events) | Print actual metric outputs for both pathways | [x] |
| 1.3 | MEDIUM | Verification | Audit and verify Planner, Critic, Scheduler, Memory, Recovery, and Determinism subsystems | Regression tests pass under `./ct` and benchmark | [x] |
| 1.4 | MEDIUM | Code Quality | Audit workspace for dead code, TODOs, and FIXMEs | `cargo check`, `cargo clippy`, and codebase searches are clean | [x] |
