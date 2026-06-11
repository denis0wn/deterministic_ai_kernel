# Learning Layer Implementation Plan

## Purpose
This document defines the implementation order for the learning layer while preserving the deterministic core as immutable.

## Exact implementation order
1. finalize registry schemas
2. finalize storage layout and naming conventions
3. implement dataset manifest and row schema definitions
4. implement dataset builder and validator
5. implement adapter registry persistence
6. implement training run manifest persistence
7. implement evaluation report persistence
8. implement approval record persistence
9. implement routing registry persistence
10. implement staged rollout commands
11. implement rollback commands
12. add end-to-end non-core integration tests

## Required Rust modules
Recommended new modules, all outside execution core:
- `src/learning/mod.rs`
- `src/learning/dataset.rs`
- `src/learning/filters.rs`
- `src/learning/validation.rs`
- `src/learning/scoring.rs`
- `src/learning/training_runs.rs`
- `src/learning/adapters.rs`
- `src/learning/evaluation.rs`
- `src/learning/approval.rs`
- `src/learning/routing.rs`

Recommended adjacent extension points:
- minimal extension around `src/model_registry.rs` for registry compatibility only[cite:80]
- policy-layer integration surfaces kept external to execution core[cite:79]

## Required CLI commands
Recommended CLI command groups:
- `learning dataset build`
- `learning dataset validate`
- `learning train start`
- `learning train inspect`
- `learning evaluate run`
- `learning approve`
- `learning route show`
- `learning route activate`
- `learning rollback`

These commands must operate on learning-layer metadata and artifacts only.

## Required tests
### Fast tests
- registry schema validation tests
- dataset row schema tests
- adapter manifest parsing tests
- routing manifest validation tests
- approval record validation tests

### Medium tests
- dataset build from sample capsule fixtures
- evaluation report generation from sample adapter metadata
- routing activation/rollback manifest flow
- staged rollout metadata transitions

### Slow tests
- trainer wrapper integration against local LM backend
- evaluation corpus runs
- adapter lifecycle end-to-end rehearsal

## Migration path
### Phase A
Introduce learning-layer file formats and manifests only. No runtime routing changes.

### Phase B
Introduce offline dataset builder and validator.

### Phase C
Introduce adapter registry and training/evaluation report persistence.

### Phase D
Introduce approval and staged routing metadata.

### Phase E
Activate learning-layer CLI flows under explicit operational control.

## Assumptions
- no execution-core changes are allowed in this implementation track
- model-routing metadata can evolve independently from workflow execution
- LoRA artifacts remain external to deterministic replay semantics

## Risks
- attempting to mix learning commands into core operational CLI without clear namespace separation
- allowing registry persistence to drift from storage layout conventions
- implementing rollout logic before evaluation and approval persistence are stable

## Unresolved questions
- exact serialization format choices for each manifest
- whether learning-layer state should remain filesystem-first or later move to a metadata database
- whether task taxonomy is identical to current model-purpose taxonomy or must expand

## Recommended first coding milestone
Milestone 1: implement manifest schemas and validation for datasets, adapters, evaluations, approvals, and routing, with no training execution and no deployment activation yet.
