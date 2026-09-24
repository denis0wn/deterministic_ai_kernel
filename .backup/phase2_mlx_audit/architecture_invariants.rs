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
