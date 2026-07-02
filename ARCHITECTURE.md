# Architecture

## Module Responsibilities

- `kernel_types` — shared data types (`ReplayCapsule`, `TrustContext`, `TrustLevel`); imported directly by other modules.
- `event_bus` — persists execution events and semantic artifacts; owns the append-only event log.
- `execution` — performs runtime work only; reads from event_bus, writes execution outcomes.
- `scheduler` — selects the next pending step only; no ownership of leases or execution.
- `worker` — owns leases and step execution lifecycle; authorises transitions via lease claims.
- `workflow` — defines canonical task classes, step kinds, flow contracts, and capability mappings. Contracts are metadata, not runtime guards.
- `replay` — rebuilds task state from the persisted event stream; operates against event_bus log.
- `snapshot` — captures and restores point-in-time replay state; versioned (v1 current, v2 reserved).
- `lm_control` — manages LM role routing, model manifest, and switch policy; exposes doctor/dry-run/safe-switch CLI surface.
- `semantic_bias` — deterministic step-ordering bias; seeded, replay-safe, versioned (v1 stable).
- `api` — public facade over event_bus, kernel_types, and replay; entry point for external callers.
- `cli_json` — structured JSON output layer for all CLI commands; schema versioned via cli-json-v1.

## Architectural Invariants

- execution is execution-only — no scheduler or ownership logic.
- scheduler is selection-only — no execution or lease management.
- worker is ownership-only — no direct scheduling decisions.
- event_bus is persistence-only — no business logic.
- LM routing is deterministic and governed by lm_control policy, not by callers.
- Semantic bias is replay-safe: same seed + input always produces same order.

## Verification Pipeline

The project uses a DAG-based verification graph (fast and deep pipelines):

    integrityV1 (preflight)
        snapshotVersionMatrixV1 (contract)
        goldenReplayCorpusV1 (contract)
        schedulerIntegrationV1 (contract)

All nodes must pass with ok: true before a phase is considered complete.

## Phase Status

- Phase 1 DONE — Core types, event bus, scheduler, worker, replay engine.
- Phase 2 DONE — pub(crate) refactor, public facade via api.rs, integration tests migrated. No regressions (56 unit + 61 binary + full integration suite green).
- Phase 3 IN PROGRESS — #7 semantic_bias_v1.schema.json, #8 BiasVersion sealed enum, #9 registry/query API.
- Phase 4 PLANNED — regression lock, architecture invariants, drift detection.
