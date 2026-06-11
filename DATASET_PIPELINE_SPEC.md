# Dataset Pipeline Specification

## Purpose
This specification defines how deterministic execution artifacts are transformed into learning datasets for offline LoRA training without altering deterministic system behavior.[conversation_history:1]

## Source artifacts
Learning data originates from:
- replay capsules
- execution traces
- evaluation outputs
- execution outcomes
- verdict and quality artifacts

These sources are read-only inputs to the learning layer.[conversation_history:1]

## Capsule to dataset transformation
The dataset builder transforms each eligible capsule or trace bundle into one or more task-scoped training examples.

Transformation stages:
1. ingest capsule and associated outcomes
2. derive task type
3. extract prompt/context summary
4. extract desired response or target label
5. attach outcome quality and provenance metadata
6. serialize to dataset row format

Example row fields:
- `example_id`
- `source_capsule_id`
- `task_type`
- `input_text`
- `target_text`
- `outcome_label`
- `quality_score`
- `provenance`
- `split`

## Filtering
Filtering removes unstable or unsafe samples before training.

Required filters:
- missing provenance filter
- failed capsule reconstruction filter
- low-confidence outcome filter
- contradictory target filter
- unsafe content / policy violation filter
- deterministic-boundary leakage filter

## Validation
Validation confirms dataset structural and semantic integrity.

Validation checks:
- schema completeness
- non-empty input and target fields
- allowed task type set
- valid provenance reference
- split integrity
- duplicate detection
- data leakage prevention between train and holdout

## Quality scoring
Each example should receive a quality score derived from:
- execution success/failure outcome
- evaluator verdicts
- consistency across repeated runs
- human label confidence if available
- regression relevance for the target task type

Dataset-level quality summary should include:
- total examples
- accepted vs rejected counts
- average quality score
- task-type distribution
- holdout coverage

## Dataset versioning
Each dataset build must be immutable and versioned.

Dataset version fields:
- `dataset_id`
- `source_window`
- `builder_version`
- `filter_policy_version`
- `schema_version`
- `created_at`
- `example_count`
- `quality_summary`

A new dataset version must be created whenever:
- source selection changes
- filtering rules change
- scoring logic changes
- schema changes

## Assumptions
- capsules and outcomes are stable enough to serve as source artifacts.[conversation_history:1]
- task types can be derived from existing purpose/routing surfaces or adjacent metadata.[cite:79][cite:80]

## Risks
- poor source labeling may produce noisy supervision
- excessive filtering may starve niche task classes
- insufficient leakage controls may contaminate evaluation splits

## Unresolved questions
- exact row format for LM Studio-compatible training backend
- whether datasets are stored as JSONL, parquet, or manifest + shards
- whether human-reviewed gold sets exist or must be added later

## Recommended implementation order
1. define canonical row schema
2. define filter policy
3. define validation runner
4. define scoring rules
5. define dataset manifest format
