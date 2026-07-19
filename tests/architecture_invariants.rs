//! Phase 4 §2 — Architecture Invariants
//!
//! Enforces forbidden dependency edges between architectural layers.
//! If any forbidden import is detected at compile time via these checks,
//! the test fails — preventing architectural drift over time.

/// semantic layer must never import cli_json
#[test]
fn semantic_must_not_import_cli_json() {
    let src = include_str!("../src/workflow/semantic/bias.rs");
    assert!(
        !src.contains("cli_json"),
        "semantic layer must not import cli_json"
    );
}

/// semantic layer must never import api
#[test]
fn semantic_must_not_import_api() {
    let src = include_str!("../src/workflow/semantic/bias.rs");
    assert!(
        !src.contains("use crate::api"),
        "semantic layer must not import api"
    );
}

/// registry must never import mutable scheduler state
#[test]
fn registry_must_not_import_scheduler() {
    let src = include_str!("../src/registry/mod.rs");
    assert!(
        !src.contains("use crate::scheduler"),
        "registry must not import scheduler (mutable state)"
    );
}

/// registry must never import worker
#[test]
fn registry_must_not_import_worker() {
    let src = include_str!("../src/registry/mod.rs");
    assert!(
        !src.contains("use crate::worker"),
        "registry must not import worker"
    );
}

/// snapshot must never import execution layer
#[test]
fn snapshot_must_not_import_execution() {
    let src = include_str!("../src/snapshot.rs");
    assert!(
        !src.contains("use crate::execution"),
        "snapshot must not import execution layer"
    );
}

/// kernel_types must never import api (transport boundary)
#[test]
fn kernel_types_must_not_import_api() {
    let src = include_str!("../src/kernel_types.rs");
    assert!(
        !src.contains("use crate::api"),
        "kernel_types must not import api — violates transport boundary"
    );
}

#[test]
fn execute_changes_must_require_tool_execution() {
    use deterministic_ai_kernel::execution_abi::primitives::PrimitiveKind;
    use deterministic_ai_kernel::workflow::contract::StepKind;

    assert_eq!(
        StepKind::ExecuteChanges.to_primitive_kind(),
        PrimitiveKind::ToolExecution
    );
    assert_ne!(
        StepKind::ExecuteChanges.to_primitive_kind(),
        PrimitiveKind::Compute
    );
    assert_ne!(
        StepKind::ExecuteChanges.to_primitive_kind(),
        PrimitiveKind::Reasoning
    );
}

#[test]
fn run_tests_must_lower_to_compute() {
    use deterministic_ai_kernel::execution_abi::primitives::PrimitiveKind;
    use deterministic_ai_kernel::workflow::contract::StepKind;

    assert_eq!(
        StepKind::RunTests.to_primitive_kind(),
        PrimitiveKind::Compute
    );
}

#[test]
fn analyze_and_plan_must_remain_reasoning_only() {
    use deterministic_ai_kernel::execution_abi::primitives::PrimitiveKind;
    use deterministic_ai_kernel::workflow::contract::StepKind;

    assert_eq!(
        StepKind::AnalyzeTask.to_primitive_kind(),
        PrimitiveKind::Reasoning
    );
    assert_eq!(
        StepKind::PlanExecution.to_primitive_kind(),
        PrimitiveKind::Reasoning
    );
}

#[test]
fn tool_like_steps_must_require_concrete_primitive_binding() {
    use deterministic_ai_kernel::workflow::compiler::step_requires_concrete_primitive_binding;
    use deterministic_ai_kernel::workflow::contract::StepKind;

    assert!(step_requires_concrete_primitive_binding(
        &StepKind::ExecuteChanges
    ));
    assert!(step_requires_concrete_primitive_binding(
        &StepKind::RunTests
    ));
    assert!(step_requires_concrete_primitive_binding(
        &StepKind::PatchCode
    ));
    assert!(!step_requires_concrete_primitive_binding(
        &StepKind::AnalyzeTask
    ));
    assert!(!step_requires_concrete_primitive_binding(
        &StepKind::PlanExecution
    ));
}

#[test]
fn primitive_bound_steps_cannot_validate_as_reasoning_only() {
    use deterministic_ai_kernel::execution_abi::primitives::PrimitiveKind;
    use deterministic_ai_kernel::planner_pipeline::execution_engine::validate_execution_receipt;
    use deterministic_ai_kernel::workflow::contract::StepKind;

    let err = validate_execution_receipt(&StepKind::ExecuteChanges, PrimitiveKind::Reasoning, None)
        .expect_err("ExecuteChanges must not validate as reasoning-only");

    let msg = err.to_string();
    assert!(
        msg.contains("primitive-bound step"),
        "unexpected error: {msg}"
    );
}

#[test]
fn primitive_bound_steps_require_materialized_artifact() {
    use deterministic_ai_kernel::execution_abi::primitives::PrimitiveKind;
    use deterministic_ai_kernel::planner_pipeline::execution_engine::validate_execution_receipt;
    use deterministic_ai_kernel::workflow::contract::StepKind;

    let err = validate_execution_receipt(&StepKind::RunTests, PrimitiveKind::Read, None)
        .expect_err("RunTests must require an artifact hash");

    let msg = err.to_string();
    assert!(
        msg.contains("materialized execution artifact"),
        "unexpected error: {msg}"
    );
}

#[test]
fn step_struct_must_expose_primitive_binding_metadata() {
    let src = include_str!("../src/workflow/contract.rs");
    assert!(
        src.contains("pub primitive_binding: Option<String>"),
        "Step must expose explicit primitive_binding metadata"
    );
}

#[test]
fn compiler_must_not_use_detail_as_binding_proxy() {
    let src = include_str!("../src/workflow/compiler.rs");
    assert!(
        src.contains("step.primitive_binding.is_none()"),
        "compiler must validate primitive_binding directly"
    );
    assert!(
        !src.contains("step.detail.is_none()"),
        "compiler must not use detail as a binding proxy"
    );
}

#[test]
fn compiler_must_construct_unbound_steps_explicitly() {
    let src = include_str!("../src/workflow/compiler.rs");
    assert!(
        src.contains("primitive_binding: None"),
        "compiler must initialize primitive_binding explicitly for newly created steps"
    );
}

#[test]
fn compiler_step_dedup_must_ignore_binding_metadata() {
    let src = include_str!("../src/workflow/compiler.rs");
    assert!(
        src.contains("existing.kind == step.kind && existing.detail == step.detail"),
        "compiler step deduplication must ignore primitive binding metadata"
    );
}

#[test]
fn llm_compile_path_must_enforce_primitive_binding() {
    let src = include_str!("../src/workflow/compiler.rs");
    assert!(
        src.contains(
            "llm compile path requires concrete primitive binding before executable spec emission"
        ),
        "LLM compile path must enforce primitive binding before emitting ExecSpec"
    );
    assert!(
        src.contains("step.primitive_binding.is_none()"),
        "LLM compile path must validate primitive_binding directly"
    );
}

#[test]
fn compiler_paths_must_enforce_primitive_binding_symmetrically() {
    let src = include_str!("../src/workflow/compiler.rs");

    assert!(
        src.contains("workflow compile invariant violated: executable spec emitted without required primitive binding"),
        "default compile path must fail closed on missing primitive binding"
    );

    assert!(
        src.contains(
            "llm compile path requires concrete primitive binding before executable spec emission"
        ),
        "LLM compile path must fail closed on missing primitive binding"
    );

    let occurrences = src
        .matches("step_requires_concrete_primitive_binding(&step.kind)")
        .count();
    assert!(
        occurrences >= 2,
        "both compiler paths must enforce primitive binding; found {occurrences} occurrences"
    );
}

#[test]
fn binding_logic_must_not_override_primitive_kind_from_detail_text() {
    let src = include_str!("../src/workflow/contract.rs");

    assert!(
        !src.contains("detail_lower.contains(\"read file\") || detail_lower.contains(\"read \")"),
        "binding logic must not override primitive kind from read-text heuristics"
    );
    assert!(
        !src.contains("detail_lower.contains(\"write file\")"),
        "binding logic must not override primitive kind from write-text heuristics"
    );
    assert!(
        !src.contains("detail_lower.contains(\"run command\")"),
        "binding logic must not override primitive kind from command-text heuristics"
    );
    assert!(
        src.contains("let prim_kind = primitive_kind;"),
        "binding logic must preserve primitive kind derived from StepKind"
    );
}
