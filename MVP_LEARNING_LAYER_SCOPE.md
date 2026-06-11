# MVP Learning Layer Scope

## Goal
The MVP introduces the learning layer as passive infrastructure only. It must exist alongside the deterministic kernel without affecting execution behavior, replay semantics, workflow orchestration, or runtime model selection.[conversation_history:1][cite:84][cite:85]

## Included in MVP
The MVP includes only the smallest passive surfaces needed to represent learning metadata and artifacts.

### 1. Dataset manifest schema
The MVP may define and persist dataset manifests derived from capsules and execution artifacts, but only as metadata. No actual training dataset execution pipeline is required beyond read-only transformation bookkeeping.[cite:84][cite:85]

Included fields should cover:
- dataset id
- source capsule references
- provenance window
- builder/schema version
- split manifest metadata
- quality summary placeholder
- creation timestamp

### 2. Dataset storage structure
The MVP may create filesystem layout for learning artifacts and dataset metadata only. This is limited to passive directory structure and manifest persistence, as already specified in the storage layout documents.[cite:84][cite:85]

### 3. Model / adapter registry schema
The MVP may define registry manifests for:
- immutable base model records
- LoRA adapter records
- version lineage and rollback references

This is metadata only. No model loading, training, activation, or routing mutation is included.[cite:80][cite:85]

### 4. Evaluation stub interface
The MVP may define an evaluation report schema or stub interface that records placeholder results and status fields. It must not execute real benchmark suites yet.[cite:85]

### 5. Approval state machine
The MVP may define manual approval metadata and a minimal approval lifecycle such as:
- draft
- under_review
- approved
- rejected

This remains human-simulated and artifact-driven only.[cite:84][cite:85]

### 6. Read-only integration with existing kernel artifacts
The MVP may read existing capsules, traces, evaluation outputs, and execution outcomes as inputs for manifest generation. Integration must remain strictly read-only.[conversation_history:1][cite:84]

## Explicitly excluded from MVP
The following are out of scope and must not be implemented in the MVP:
- LoRA training execution[conversation_history:1]
- LM Studio training/inference integration calls[cite:79]
- automatic or manual weight mutation of base models[conversation_history:1]
- runtime routing changes in live execution[cite:85]
- production deployment activation[cite:85]
- benchmark execution beyond stub manifests[cite:85]
- any execution-core or orchestration changes[conversation_history:1]

## MVP boundary statement
The learning layer MVP is passive infrastructure only. It stores manifests, registries, and approval metadata, and may read historical artifacts to populate them. It has no authority to change runtime execution behavior.[cite:84][cite:85]
