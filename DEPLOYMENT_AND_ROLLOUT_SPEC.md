# Deployment and Rollout Specification

## Purpose
This specification compares rollout models for LoRA adapter deployment and recommends a safe approach that preserves deterministic execution boundaries.

## Rollout options

### Single active adapter
One adapter is active per task type and environment.

Pros:
- simplest routing model
- easiest audit trail
- easiest rollback
- minimal runtime ambiguity

Cons:
- no exposure gradient
- higher risk when activating a new adapter
- limited production learning before full switch

### Staged rollout
Adapter advances through fixed environments, e.g. local -> staging -> production.

Pros:
- clear approval checkpoints
- easy to reason about operationally
- good fit for human review workflows

Cons:
- slower promotion
- environment drift can hide edge cases

### Canary rollout
A small percentage of eligible traffic is routed to the new adapter.

Pros:
- early live signal before full promotion
- limits blast radius

Cons:
- more runtime complexity
- requires traffic partitioning and measurement discipline
- may complicate reproducibility unless treated as environment-scoped inference only

### Ring rollout
Adapters are exposed to increasingly broad rings, e.g. internal -> trusted -> broad.

Pros:
- strong operational control
- better safety than direct full rollout
- more expressive than simple staging

Cons:
- highest metadata complexity
- requires ring definitions, routing policy, and evaluation segmentation

## Recommendation
Recommended initial approach: **staged rollout with a single active adapter per task type per environment**.

### Why this is recommended
- It preserves operational simplicity.
- It minimizes ambiguity in routing records.
- It supports explicit human approval between stages.
- It keeps rollback simple: revert routing to previous adapter in the affected environment.
- It avoids introducing canary/ring complexity before the learning layer itself is stable.

## Suggested rollout path
1. candidate trained
2. offline evaluation passes
3. human approval for `local`
4. routing update in `local`
5. post-deploy observation
6. human approval for `staging`
7. routing update in `staging`
8. final approval for `production`
9. routing update in `production`

## Rollback model
Rollback is always per-environment and per-task-type. The previous routing entry must be preserved so rollback can restore the last known good adapter without retraining.

## Promotion constraints
Promotion is blocked if:
- evaluation thresholds are not met
- safety checks fail
- approval record is missing
- rollback target is not available

## Assumptions
- task-type routing can be expressed independently from execution orchestration
- environments are meaningful for the deployment surface even if the deterministic core is unchanged

## Risks
- direct introduction of canary/ring rollout may overcomplicate early implementation
- missing environment separation may cause accidental broad rollout
- insufficient routing audit history may weaken rollback reliability

## Unresolved questions
- whether local/staging/production are separate LM Studio instances or logical scopes within one instance
- whether future ring rollout is needed for high-risk task classes such as planner or policy suggestion
