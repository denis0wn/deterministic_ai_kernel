# Learning Layer Architecture

## Purpose
The learning layer is a non-deterministic, offline adaptation system that improves model behavior through versioned LoRA adapters while preserving the deterministic execution kernel as immutable. This layer operates above the execution core and does not participate in replay semantics, event transitions, workflow execution, or orchestration logic.[conversation_history:1][cite:79][cite:80]

## System boundaries

### Immutable core boundary
The following surfaces are treated as immutable during this phase:
- execution kernel
- replay engine
- snapshot system
- event model
- orchestration / workflow execution
- CLI contracts

These boundaries already exist in the project context and must not be modified by the learning layer.[conversation_history:1]

### Adaptive model boundary
The adaptive boundary includes only model-facing surfaces:
- model registry / model purpose resolution
- external LM policy layer
- LM Studio model backend integration
- LoRA training and deployment metadata

The repository already contains LM-related surfaces such as `src/model_registry.rs`, `src/model_manifest.rs`, `src/lm_control.rs`, and `src/lm_policy_layer/*`, which provide the natural integration boundary for a learning layer without entering deterministic execution paths.[cite:79][cite:80]

## High-level data flow
1. The deterministic system produces capsules, traces, evaluations, and execution outcomes.
2. The learning layer ingests those outputs asynchronously outside execution-time control flow.
3. Capsule-derived examples are filtered and transformed into versioned datasets.
4. A LoRA training pipeline runs against an immutable base model.
5. Candidate LoRA adapters are evaluated offline.
6. Successful candidates are written into a versioned adapter registry.
7. Human approval is required before routing a new adapter into active inference.
8. Deployment changes only model routing metadata; kernel behavior remains unchanged.

## Capsule ingestion
Capsules are the preferred structured source for learning inputs because they already summarize execution-relevant context without modifying deterministic state. Capsule ingestion should read from saved replay capsules, execution traces, evaluation outputs, and outcome verdicts, then normalize them into a learning-event envelope.

A learning-event envelope should contain:
- task type
- capsule identifier
- execution identifier
- input summary
- decision trace summary
- output summary
- outcome label
- confidence / evaluation score
- provenance metadata

## Dataset generation
Dataset generation converts learning-event envelopes into task-specialized supervised training examples. Generation must be purpose-aware rather than execution-aware: the dataset builder should map examples into categories such as planning, classification, summarization, or policy suggestion, but never into state transition logic.

The output should be a versioned dataset artifact with:
- dataset id
- source capsule set
- transformation rules version
- filter policy version
- quality score summary
- train / validation / holdout split manifest

## Training pipeline
The training pipeline is an offline continual fine-tuning loop:
1. choose immutable base model
2. choose a task-specialized dataset version
3. run LoRA fine-tuning through LM Studio-compatible backend tooling
4. emit adapter artifact only, never modify base weights
5. register adapter with version metadata and provenance

Training outputs must include:
- adapter id
- base model id
- dataset id
- training config hash
- creation timestamp
- metrics summary
- artifact path or registry URI

## Evaluation pipeline
Every trained adapter must pass offline evaluation before any deployment decision. Evaluation should be isolated from live execution and should include:
- holdout task evaluation
- regression evaluation against prior adapter
- policy safety evaluation
- deterministic-boundary compliance check
- quality threshold gating

Evaluation results should be written as immutable reports linked to the adapter version.

## Registry structure
The learning layer requires two logical registries.

### Base model registry
This registry records immutable base models and their compatibility surface. The current repository already has a base model resolution layer in `src/model_registry.rs`; the learning architecture should extend registry metadata around that boundary rather than replacing it.[cite:80]

Base model record fields:
- base_model_id
- provider/backend
- base model name
- format/runtime compatibility
- supported task classes
- status: active | deprecated | retired

### LoRA adapter registry
This registry records versioned adapters independent of routing state.

Adapter record fields:
- adapter_id
- task_type
- base_model_id
- dataset_id
- evaluation_report_id
- approval status
- deployment status
- rollback predecessor
- created_at
- artifact location

### Routing registry
Routing metadata must be separate from both base model and adapter registries.

Routing record fields:
- task_type
- selected base_model_id
- selected adapter_id (optional)
- deployment ring / environment
- approved_by
- activated_at

This separation ensures that learning lifecycle and routing lifecycle are independent.

## Rollback strategy
Rollback applies to adapter deployment, not to core execution. Every deployed adapter must preserve:
- previous active adapter reference
- previous routing entry
- evaluation lineage
- approval lineage

Rollback procedure:
1. freeze promotion of candidate adapter
2. restore prior routing entry
3. mark current adapter deployment as rolled back
4. preserve both adapter artifacts and evaluation history

Rollback must never require retraining and must never mutate historical datasets or evaluation records.

## Deployment flow
1. candidate adapter produced by training
2. offline evaluation completed
3. human review of evaluation report
4. approval gate passed
5. routing metadata updated to point task type to new adapter
6. canary / staged exposure if desired
7. post-deployment evaluation observed outside deterministic core
8. if degraded, rollback via routing registry only

## Safety constraints
The learning layer may:
- read capsules, traces, evaluation reports, and execution outcomes
- build datasets from historical artifacts
- train LoRA adapters against immutable base models
- write registry metadata for datasets, adapters, and routing
- propose routing changes subject to approval

The learning layer may not:
- modify execution kernel behavior
- change replay semantics
- modify event schemas or event transitions
- mutate workflow execution logic
- alter orchestration policy inside the deterministic core
- rewrite prior capsules, traces, or historical outcomes
- directly edit base model weights

## Assumptions
- LM Studio remains the local model-serving boundary.[cite:79]
- Existing `model_registry` and LM policy surfaces are the intended integration boundary for adaptive model management.[cite:79][cite:80]
- Capsules, traces, and execution outcomes are already available as historical artifacts from the existing deterministic system.[conversation_history:1]

## Risks
- leakage of execution semantics into training labels could unintentionally couple learning outputs to deterministic state transitions
- insufficient dataset filtering may train unstable or low-quality adapters
- adapter routing drift may occur if routing and registry responsibilities are not separated
- unreviewed deployment of new adapters may degrade planning or policy quality

## Unresolved questions
- exact on-disk or registry format for adapter artifacts
- exact evaluation benchmark suite per task type
- exact approval workflow owner and sign-off process
- whether deployment should support ring-based rollout or only single active adapter per task type

## Recommended implementation order
1. dataset pipeline specification
2. LoRA lifecycle specification
3. safety boundary enforcement specification
4. registry schema extension design
5. offline trainer wrapper
6. evaluation runner
7. routing integration with approval gate
