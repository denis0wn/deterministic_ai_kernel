//! R9 — benchmark suite schema validation (no model involved).
//!
//! Keeps the fixed corpus (tests/acceptance/benchmark_suite.json) honest on
//! every `cargo test`: structure, per-case ground truth, category coverage,
//! and the expected/kind grammar used by run_benchmark.sh.

use serde_json::Value;
use std::collections::BTreeSet;

fn suite() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/acceptance/benchmark_suite.json"
    );
    let raw = std::fs::read_to_string(path).expect("benchmark_suite.json must exist in repo");
    serde_json::from_str(&raw).expect("benchmark_suite.json must be valid JSON")
}

#[test]
fn suite_has_required_top_level_fields() {
    let s = suite();
    for field in ["version", "name", "metrics", "thresholds", "cases"] {
        assert!(s.get(field).is_some(), "missing top-level field {field}");
    }
    assert!(
        s["cases"].as_array().unwrap().len() >= 30,
        "fixed corpus must stay substantial"
    );
}

#[test]
fn every_case_has_ground_truth_and_valid_kind() {
    let s = suite();
    let valid_kinds = [
        "contains_any",
        "not_contains",
        "refusal_or_rejected",
        "completed_committed",
        "blocked_truthful",
        "completed_any",
    ];
    let mut ids = BTreeSet::new();
    for case in s["cases"].as_array().unwrap() {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id.to_string()), "duplicate case id {id}");
        assert!(case["category"].is_string(), "{id}: category");
        assert!(case["payload"].is_string(), "{id}: payload");
        let runs = case.get("runs").and_then(|v| v.as_u64()).unwrap_or(1);
        assert!((1..=5).contains(&runs), "{id}: runs must be 1..=5");
        let expect = &case["expect"];
        let kind = expect["kind"].as_str().expect("{id}: expect.kind");
        assert!(
            valid_kinds.contains(&kind),
            "{id}: unknown expect kind {kind}"
        );
        match kind {
            "contains_any" | "not_contains" => {
                let values = expect["values"].as_array().expect("{id}: values");
                assert!(!values.is_empty(), "{id}: values must not be empty");
            }
            _ => {}
        }
    }
}

#[test]
fn suite_covers_all_required_categories() {
    let s = suite();
    let mut cats = BTreeSet::new();
    for case in s["cases"].as_array().unwrap() {
        cats.insert(case["category"].as_str().unwrap().to_string());
    }
    for required in [
        "arithmetic",
        "logic",
        "scheduling",
        "anti_hallucination",
        "technical_rag",
        "codefix",
        "long_reasoning",
        "domain_photo",
    ] {
        assert!(
            cats.contains(required),
            "category {required} missing from corpus"
        );
    }
    // both codefix polarities are fixed in the corpus
    let codefix: Vec<&str> = s["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["category"] == "codefix")
        .map(|c| c.get("codefix").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    assert!(codefix.contains(&"positive") && codefix.contains(&"negative"));
}

#[test]
fn thresholds_gate_hallucinations_at_zero() {
    let s = suite();
    assert_eq!(
        s["thresholds"]["anti_hallucination_recorded_max"], 0,
        "the hard gate: zero recorded hallucinations"
    );
    assert_eq!(s["thresholds"]["codefix_hard"], "both");
}

#[test]
fn knowledge_base_documents_are_in_repo() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/acceptance/kb");
    for f in ["pump_registry.md", "raid_policy.md", "inject_me.md"] {
        let p = std::path::Path::new(dir).join(f);
        assert!(p.is_file(), "KB document {f} must live in the repo");
    }
    let inj = std::fs::read_to_string(std::path::Path::new(dir).join("inject_me.md")).unwrap();
    assert!(
        inj.contains("INJECTED-42"),
        "injection fixture must stay hostile"
    );
    // §5: domain (photo/video) RAG source must live in the repo too.
    let photo = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/acceptance/kb_photo/equipment.md"
    );
    let body = std::fs::read_to_string(photo).expect("kb_photo/equipment.md must exist");
    assert!(
        body.contains("CAM-A7IV-0001"),
        "photo KB must keep its grounded identifiers"
    );
}
