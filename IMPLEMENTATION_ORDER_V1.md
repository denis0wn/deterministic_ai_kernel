# Implementation Order V1

## Goal
This plan defines the smallest safe implementation order for introducing the learning layer as passive infrastructure only.[cite:84][cite:85]

## Order
1. Add learning-layer manifest schemas for datasets, base models, adapters, evaluation stubs, and approval records.[cite:85]
2. Add filesystem layout creation and manifest persistence helpers for passive storage only.[cite:85]
3. Add read-only loaders for existing capsules/traces/outcomes so dataset manifests can reference verified source artifacts.[cite:84]
4. Add a manual approval state machine represented purely as metadata transitions.[cite:85]
5. Add tests for schema validity, manifest persistence, and read-only source ingestion.[cite:85]

## What can be implemented in 1–2 days
A 1–2 day implementation can safely include:
- manifest struct definitions
- serialization/deserialization tests
- passive `learning/` directory layout creation
- dataset manifest writer
- adapter/base-model registry manifest writer
- evaluation stub manifest writer
- manual approval record writer
- read-only fixture-based ingestion from existing capsule-like artifacts

These items do not require runtime model control or deterministic core modification.[cite:84][cite:85]

## What must not be touched yet
The following must remain untouched in V1:
- training execution pipelines[cite:84]
- LM Studio calls[cite:79]
- routing activation logic[cite:85]
- production rollout paths[cite:85]
- replay/snapshot/event/orchestration code paths[conversation_history:1]
- any kernel behavior affecting deterministic execution[conversation_history:1]

## Exact first coding step
Single smallest safe commit:
- introduce manifest schema types for `DatasetManifest`, `BaseModelRecord`, `LoraAdapterRecord`, `EvaluationStubReport`, and `ApprovalRecord`, plus round-trip serialization tests, with no command wiring and no runtime integration yet.[cite:85]

This is the minimal safe starting point because it creates only passive data definitions and validates file format stability without touching execution behavior.[cite:84][cite:85]
