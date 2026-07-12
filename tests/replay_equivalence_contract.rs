/// ReplayEquivalence contract module — Phase 1 §3
///
/// Single authoritative place asserting the two replay guarantees:
///   1. same seed + same input  → identical snapshot graph (bit-level)
///   2. ordering independence   → same multiset of steps → same weights
///
/// These are CONTRACT tests, not property tests: they lock the observable
/// behaviour and will fail loudly if the kernel drifts.
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unique_db(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dak_reqeq_{}_{}.db", label, nanos))
        .display()
        .to_string()
}

fn cleanup(db: &str) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}

fn run_ok(db: &str, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .expect("failed to spawn kernel");
    assert!(
        out.status.success(),
        "cmd {:?} failed\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("test failure")
}

fn emit(db: &str, task: &str, step: &str, steps: &[&str]) {
    let mut args = vec!["emit-bias-artifact", task, step];
    args.extend_from_slice(steps);
    let _ = run_ok(db, &args);
}

fn fetch_payload(db: &str, task: &str, step: &str) -> Value {
    let stdout = run_ok(db, &["latest-bias-artifact", task, step]);
    let line = stdout.lines().next().expect("expected artifact row");
    let cols: Vec<&str> = line.splitn(6, '\t').collect();
    assert_eq!(cols.len(), 6, "unexpected columns: {line}");
    serde_json::from_str(cols[5]).expect("invalid JSON payload")
}

/// Snapshot graph: map of step_id → payload for all steps emitted in a task.
/// We use a single (task, step) pair per test — so the "graph" is one node.
/// Extend to multi-step when the kernel supports graph traversal.
fn snapshot_graph(db: &str, task: &str, step: &str) -> Value {
    fetch_payload(db, task, step)
}

// ---------------------------------------------------------------------------
// Contract matrix: canonical inputs used across all contract tests
// ---------------------------------------------------------------------------

struct Case {
    tag: &'static str,
    steps: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        tag: "single-analyze",
        steps: &["AnalyzeTask"],
    },
    Case {
        tag: "single-execute",
        steps: &["ExecuteChanges"],
    },
    Case {
        tag: "single-run",
        steps: &["RunTests"],
    },
    Case {
        tag: "pair-ae",
        steps: &["AnalyzeTask", "ExecuteChanges"],
    },
    Case {
        tag: "pair-ar",
        steps: &["AnalyzeTask", "RunTests"],
    },
    Case {
        tag: "triple-aer",
        steps: &["AnalyzeTask", "ExecuteChanges", "RunTests"],
    },
    Case {
        tag: "triple-all",
        steps: &[
            "AnalyzeTask",
            "PlanExecution",
            "ExecuteChanges",
            "RunTests",
            "ValidatePatch",
        ],
    },
    Case {
        tag: "chain-repeat",
        steps: &[
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
            "AnalyzeTask",
            "ExecuteChanges",
            "RunTests",
        ],
    },
];

// ---------------------------------------------------------------------------
// Contract 1: same seed + same input → identical snapshot graph
//
// Asserts: emit twice with identical args → payload is bit-for-bit identical.
// This is the "same seed + same input" guarantee expressed over the CLI contract.
// ---------------------------------------------------------------------------

#[test]
fn contract_same_input_identical_snapshot_graph() {
    for case in CASES {
        let db = unique_db(&format!("c1-{}", case.tag));
        cleanup(&db);

        let task = format!("task-c1-{}", case.tag);
        let step = "step-c1";

        emit(&db, &task, step, case.steps);
        let graph_a = snapshot_graph(&db, &task, step);

        emit(&db, &task, step, case.steps);
        let graph_b = snapshot_graph(&db, &task, step);

        assert_eq!(
            graph_a, graph_b,
            "[contract-1] snapshot graph drifted on second emit\ntag={}\nsteps={:?}",
            case.tag, case.steps
        );
        assert_eq!(graph_a["version"], "v1", "[contract-1] version must be v1");
        assert_eq!(graph_a["seed"], 0, "[contract-1] seed must be 0");

        cleanup(&db);
    }
}

// ---------------------------------------------------------------------------
// Contract 2: snapshot → restore preserves snapshot graph exactly
// ---------------------------------------------------------------------------

#[test]
fn contract_snapshot_restore_preserves_graph() {
    for case in CASES {
        let db = unique_db(&format!("c2-{}", case.tag));
        cleanup(&db);

        let task = format!("task-c2-{}", case.tag);
        let step = "step-c2";

        emit(&db, &task, step, case.steps);
        let before = snapshot_graph(&db, &task, step);

        let _ = run_ok(&db, &["snapshot", &task]);
        let _ = run_ok(&db, &["restore", &task]);

        emit(&db, &task, step, case.steps);
        let after = snapshot_graph(&db, &task, step);

        assert_eq!(
            before, after,
            "[contract-2] snapshot graph drifted after snapshot/restore\ntag={}\nsteps={:?}",
            case.tag, case.steps
        );

        cleanup(&db);
    }
}

// ---------------------------------------------------------------------------
// Contract 3: ordering independence
//
// Same multiset of steps in different order → identical weights map.
// ---------------------------------------------------------------------------

#[test]
fn contract_ordering_independence_guaranteed() {
    let ordered = &["AnalyzeTask", "ExecuteChanges", "RunTests", "ValidatePatch"];
    let reversed = &["ValidatePatch", "RunTests", "ExecuteChanges", "AnalyzeTask"];
    let shuffled = &["RunTests", "AnalyzeTask", "ValidatePatch", "ExecuteChanges"];

    let permutations: &[(&str, &[&str])] =
        &[("ord", ordered), ("rev", reversed), ("shu", shuffled)];

    let mut payloads: HashMap<&str, Value> = HashMap::new();

    for (label, steps) in permutations {
        let db = unique_db(&format!("c3-{label}"));
        cleanup(&db);
        let task = format!("task-c3-{label}");
        emit(&db, &task, "step-c3", steps);
        payloads.insert(label, fetch_payload(&db, &task, "step-c3"));
        cleanup(&db);
    }

    let w_ord = &payloads["ord"]["weights"];
    let w_rev = &payloads["rev"]["weights"];
    let w_shu = &payloads["shu"]["weights"];

    assert_eq!(
        w_ord, w_rev,
        "[contract-3] ordered vs reversed weights differ"
    );
    assert_eq!(
        w_ord, w_shu,
        "[contract-3] ordered vs shuffled weights differ"
    );
}

// ---------------------------------------------------------------------------
// Contract 4: structural schema invariants on every canonical case
// ---------------------------------------------------------------------------

#[test]
fn contract_payload_schema_invariants() {
    for case in CASES {
        let db = unique_db(&format!("c4-{}", case.tag));
        cleanup(&db);

        let task = format!("task-c4-{}", case.tag);
        let step = "step-c4";
        emit(&db, &task, step, case.steps);
        let p = fetch_payload(&db, &task, step);

        assert_eq!(p["version"], "v1", "[contract-4] version tag={}", case.tag);
        assert_eq!(p["seed"], 0, "[contract-4] seed    tag={}", case.tag);

        let weights = p["weights"]
            .as_object()
            .unwrap_or_else(|| panic!("[contract-4] weights not object tag={}", case.tag));
        assert!(
            !weights.is_empty(),
            "[contract-4] weights empty tag={}",
            case.tag
        );

        let lines = p["lines"]
            .as_array()
            .unwrap_or_else(|| panic!("[contract-4] lines not array tag={}", case.tag));
        assert!(
            !lines.is_empty(),
            "[contract-4] lines empty tag={}",
            case.tag
        );

        // Every unique step in input must appear as a weight key
        let unique: std::collections::HashSet<&str> = case.steps.iter().copied().collect();
        for s in &unique {
            assert!(
                weights.contains_key(*s),
                "[contract-4] missing weight for {s} tag={}",
                case.tag
            );
        }

        cleanup(&db);
    }
}
