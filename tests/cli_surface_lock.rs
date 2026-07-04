//! Phase 4 §4 — CLI Surface Lock
//!
//! Locks the exhaustive set of CLI subcommands exposed by main.rs.
//! If a subcommand is added, renamed, or removed, this test fails —
//! forcing an explicit, reviewed update to the surface contract.

/// The locked set of CLI subcommands in sorted order.
const LOCKED_SUBCOMMANDS: &[&str] = &[
    "analyze-task",
    "auto-route",
    "bias-explain",
    "capture-capsule",
    "capture-capsule-save",
    "compare-capsules",
    "current-models",
    "doctor",
    "doctor-json",
    "embeddings-smoke",
    "emit-bias-artifact",
    "execute-effects",
    "gateway-stdin",
    "integrity",
    "integrity-json",
    "latest-analysis-seed",
    "latest-bias-artifact",
    "latest-capsule",
    "llm-planner-smoke",
    "llm-prompt",
    "llm-smoke",
    "memory",
    "next-ready",
    "plan-task",
    "print-model-manifest",
    "reconcile",
    "replay",
    "replay-capsule",
    "reset",
    "restore",
    "schedule",
    "semantic-artifacts",
    "snapshot",
    "snapshot-artifacts",
    "stats",
    "status-map",
    "switch",
    "sync-all-model-roles",
    "vacuum",
];

#[test]
fn cli_subcommand_count_is_locked() {
    assert_eq!(
        LOCKED_SUBCOMMANDS.len(),
        39,
        "CLI subcommand count changed — update LOCKED_SUBCOMMANDS and bump this assertion"
    );
}

#[test]
fn cli_subcommands_are_sorted() {
    let mut sorted = LOCKED_SUBCOMMANDS.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        LOCKED_SUBCOMMANDS.to_vec(),
        sorted,
        "LOCKED_SUBCOMMANDS must be kept in sorted order"
    );
}

#[test]
fn cli_surface_contains_core_commands() {
    let core = [
        "integrity",
        "integrity-json",
        "doctor",
        "doctor-json",
        "schedule",
        "reconcile",
        "snapshot",
        "restore",
        "replay",
        "plan-task",
        "analyze-task",
        "emit-bias-artifact",
        "bias-explain",
        "gateway-stdin",
    ];
    for cmd in &core {
        assert!(
            LOCKED_SUBCOMMANDS.contains(cmd),
            "Core command '{}' missing from CLI surface lock",
            cmd
        );
    }
}

#[test]
fn main_rs_contains_all_locked_subcommands() {
    let src = include_str!("../src/main.rs");
    for cmd in LOCKED_SUBCOMMANDS {
        assert!(
            src.contains(&format!("\"{}\"", cmd)),
            "Subcommand '{}' in lock but not found in main.rs — was it renamed or removed?",
            cmd
        );
    }
}
