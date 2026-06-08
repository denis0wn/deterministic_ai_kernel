# Workflow ABI v1

## Purpose

This document defines the external semantic contract for task normalization and scheduler execution.

## Semantic authorities

- Compiler/ingestion is the semantic authority.
- Scheduler is the execution authority.
- Database is the persistence carrier of task semantics.

## Task identity model

A task is defined by two orthogonal fields:

- `task_id`: persistence and identity key.
- `task_class`: execution semantics key.

`task_id` must not be used to derive workflow semantics.

## Task classes

Supported `task_class` values:

- `Generic`
- `PlannerHardening`
- `CodeFix`

Unknown task classes must be rejected by the execution boundary.

## Task input normalization

Normalization rules:

- `TaskInput::Generic(_)` -> `TaskClass::Generic`
- `TaskInput::PlannerHardening(_)` -> `TaskClass::PlannerHardening`
- `TaskInput::CodeFix(CompileError)` -> `TaskClass::CodeFix`
- `TaskInput::CodeFix(TestFailure)` -> `TaskClass::CodeFix`
- `TaskInput::CodeFix(LintReport)` -> `TaskClass::CodeFix`

## Canonical flows

### Generic

1. `AnalyzeTask` -> `Planner`
2. `PlanExecution` -> `Planner`
3. `ExecuteChanges` -> `Executor`

### PlannerHardening

1. `TightenPlannerPrompt` -> `Planner`
2. `NormalizePlannerOutput` -> `Planner`
3. `AddLlmFallbackHandling` -> `Planner`
4. `AddPlannerTestCoverage` -> `Planner`
5. `ValidatePlannerOutput` -> `Verifier`

### CodeFix

1. `ReadRepository` -> `Planner`
2. `LocateBug` -> `Planner`
3. `PatchCode` -> `Executor`
4. `RunTests` -> `Executor`
5. `ValidatePatch` -> `Verifier`

## Scheduler contract

Scheduler must:

- read `task_class` from persistent storage,
- map `task_class` to the canonical flow,
- schedule only from canonical flow,
- never infer semantics from `task_id`,
- never reconstruct task type heuristically.

## Persistence contract

Persistent storage must contain:

- `task_id`
- `task_class`

`task_class` is immutable after task creation unless an explicit migration rule is defined.

## Compatibility note

This is ABI version 1.
Any future change to task classes, canonical flows, or capability mapping must increment the ABI version or define an explicit compatibility rule.
