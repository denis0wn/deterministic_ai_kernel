//! Phase 4 §4 — CLI Surface Lock
//!
//! Locks the exhaustive set of CLI subcommands exposed by main.rs.
//! If a subcommand is added, renamed, or removed, this test fails —
//! forcing an explicit, reviewed update to the surface contract.
//!
//! Rewritten during the remediation pass (audit finding: the previous lock
//! only checked its own constants, so new subcommands added to main.rs were
//! never detected — the lock had drifted 16 commands behind). The discovery
//! test below now scans main.rs and asserts the locked set equals the
//! implemented set in BOTH directions.

/// The locked set of CLI subcommands in sorted order.
const LOCKED_SUBCOMMANDS: &[&str] = &[
    "analyze-task",
    "auto-route",
    "bias-explain",
    "capture-capsule",
    "capture-capsule-save",
    "claim-worker",
    "compare-capsules",
    "complete-step",
    "current-models",
    // PROGRESS UNTIL VERIFIED stage 4: decomposition finalize record.
    "decomposition",
    "doctor",
    "doctor-json",
    "embeddings-smoke",
    "emit-bias-artifact",
    "execute-effects",
    "expire-leases",
    "fail-step",
    "gateway-stdin",
    "heartbeat",
    "integrity",
    "integrity-json",
    "latest-analysis-seed",
    "latest-bias-artifact",
    "latest-capsule",
    "llm-planner-smoke",
    "llm-prompt",
    "llm-smoke",
    // P0 MLX lifecycle: operator controls for the managed model server.
    "lm-lifecycle",
    "memory",
    "model-load",
    "model-unload",
    "models-list",
    "models-loaded",
    "next-ready",
    "pipeline-run",
    "plan-task",
    "print-model-manifest",
    // PROGRESS UNTIL VERIFIED stage 2: read-only progress ledger
    // (attempts grouped by payload fingerprint; repetition detection).
    "progress",
    "prompt",
    "reconcile",
    "replay",
    "replay-capsule",
    "reset",
    "restore",
    "rmdb",
    "schedule",
    "seed-leases",
    "semantic-artifacts",
    "smart-switch",
    "snapshot",
    "snapshot-artifacts",
    "start-step",
    "stats",
    "status-map",
    "submit-task",
    "switch",
    "sync-all-model-roles",
    "vacuum",
];

/// Discover subcommand match arms in main.rs: `Some("name") =>`.
fn discovered_subcommands() -> std::collections::BTreeSet<String> {
    let src = include_str!("../src/main.rs");
    let re = regex::Regex::new(r#"Some\("([a-z][a-z0-9-]*)"\) =>"#).expect("regex compiles");
    re.captures_iter(src).map(|c| c[1].to_string()).collect()
}

#[test]
fn cli_subcommand_count_is_locked() {
    assert_eq!(
        LOCKED_SUBCOMMANDS.len(),
        58, // PROGRESS UNTIL VERIFIED stage 4: +1 for decomposition
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
    // No duplicates.
    let set: std::collections::BTreeSet<&str> = LOCKED_SUBCOMMANDS.iter().copied().collect();
    assert_eq!(
        set.len(),
        LOCKED_SUBCOMMANDS.len(),
        "LOCKED_SUBCOMMANDS contains duplicates"
    );
}

#[test]
fn locked_equals_discovered_both_directions() {
    let locked: std::collections::BTreeSet<String> =
        LOCKED_SUBCOMMANDS.iter().map(|s| s.to_string()).collect();
    let discovered = discovered_subcommands();

    let missing_from_main: Vec<&String> = locked.difference(&discovered).collect();
    assert!(
        missing_from_main.is_empty(),
        "Locked subcommands not implemented in main.rs (renamed/removed?): {missing_from_main:?}"
    );

    let missing_from_lock: Vec<&String> = discovered.difference(&locked).collect();
    assert!(
        missing_from_lock.is_empty(),
        "Subcommands implemented in main.rs but absent from the lock (add them after review): {missing_from_lock:?}"
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
        "pipeline-run",
        "submit-task",
        "execute-effects",
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
