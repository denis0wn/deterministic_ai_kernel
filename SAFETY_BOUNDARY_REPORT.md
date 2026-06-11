# Safety Boundary Report

## Purpose
This report defines the hard boundary between the deterministic execution system and the adaptive learning layer.

## What the learning layer may observe
The learning layer may observe only historical and externalized artifacts such as:
- replay capsules
- execution traces
- evaluation outputs
- execution outcomes
- model registry metadata
- policy-layer metadata

These are observational surfaces only and are consumed outside deterministic execution flow.[conversation_history:1][cite:79][cite:80]

## What the learning layer may modify
The learning layer may modify only adaptive-layer metadata and artifacts:
- dataset manifests
- dataset quality reports
- LoRA training outputs
- LoRA adapter registry records
- routing metadata after approval
- evaluation reports for candidate adapters

These modifications must occur outside replay, event transitions, and execution orchestration.[conversation_history:1]

## What the learning layer may never modify
The learning layer may never modify:
- execution kernel
- workflow execution logic
- replay semantics
- event model
- snapshot semantics
- orchestration state machine
- CLI contract semantics
- historical capsules
- historical traces
- historical evaluation records
- base model weights

## Immutable boundary statement
The execution system is the source of truth for execution behavior. The learning layer is only a producer of candidate model artifacts and routing suggestions. It has no authority over deterministic state transitions.[conversation_history:1]

## Human approval boundary
No newly trained adapter may become active without human approval. Approval must review:
- dataset provenance
- evaluation results
- regression comparison
- safety boundary compliance
- rollback readiness

## Rollback boundary
Rollback of a deployed adapter must be performed by routing reversal only. It must never require mutation of kernel state, replay history, or historical model artifacts.

## Assumptions
- policy layer remains an external adaptive configuration surface rather than a core execution authority.[conversation_history:1][cite:79]
- model routing can be separated from learning lifecycle using registry metadata.[cite:79][cite:80]

## Risks
- accidental routing authority creep into execution code
- insufficient separation between policy suggestion and execution enforcement
- deployment shortcuts that bypass approval and rollback metadata

## Unresolved questions
- exact enforcement mechanism for approval gates
- exact storage location for adapter and dataset registries
- whether routing changes are environment-scoped or globally scoped

## Recommended implementation order
1. formalize immutable boundary in registry/spec docs
2. define approval metadata fields
3. define rollback metadata fields
4. define runtime checks ensuring routing cannot mutate core semantics
