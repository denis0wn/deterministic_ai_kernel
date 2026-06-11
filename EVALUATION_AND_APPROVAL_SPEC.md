# Evaluation and Approval Specification

## Purpose
This specification defines the benchmark process, metrics, thresholds, rejection criteria, and approval workflow for LoRA adapter promotion.

## Benchmark process
Each candidate adapter must be evaluated offline before any deployment step.

Required benchmark stages:
1. static schema validation
2. artifact integrity validation
3. holdout benchmark evaluation
4. regression comparison against current active adapter
5. safety policy benchmark
6. recommendation synthesis

## Evaluation metrics
Metrics should be task-type specific, but the evaluation framework should always include:
- task success rate
- regression delta vs active adapter
- error rate
- abstention / uncertainty behavior if applicable
- policy or safety violation rate
- benchmark coverage size

Optional task-specific metrics:
- plan quality score
- classification F1
- summarization faithfulness score
- route recommendation precision

## Promotion thresholds
Promotion requires all mandatory gates to pass.

Recommended default gates:
- no critical safety violations
- non-negative regression against active adapter on mandatory benchmarks
- minimum benchmark coverage reached
- dataset provenance complete
- rollback target exists
- human approval recorded

## Rejection criteria
Automatic rejection should occur if any of the following holds:
- artifact integrity mismatch
- missing provenance
- failed benchmark schema
- safety violation above threshold
- regression beyond tolerated delta
- benchmark sample count below minimum

Manual rejection may occur if:
- qualitative review identifies unstable behavior
- task specialization is too narrow
- evaluation explains gains only through unsafe overfitting

## Approval workflow
Recommended approval flow:
1. evaluator produces immutable evaluation report
2. reviewer checks benchmark results and regression summary
3. reviewer checks safety report
4. reviewer confirms rollback readiness
5. reviewer records approval or rejection decision
6. approved adapter becomes eligible for staged rollout

## Approval record contents
- approval_record_id
- adapter_id
- evaluation_report_id
- decision: approved | rejected | approved_with_scope
- approver_id
- environment_scope
- timestamp
- notes

## Assumptions
- evaluation is always offline-first
- human approval is mandatory prior to routing changes
- adapter deployment is separate from training completion

## Risks
- weak benchmark sets may approve adapters that look good only on narrow data
- missing regression comparison may hide degradation in important task slices
- unclear approval ownership may create process bypasses

## Unresolved questions
- exact threshold values per task type
- whether approval requires one approver or dual sign-off for high-risk task classes
- whether production promotion requires stronger thresholds than local/staging
