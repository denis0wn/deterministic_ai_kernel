//! Coverage test for the multi-file `pilot_billing_service` fixture and
//! the two new financial rules (`float-equality`, `floor-div-money`).
//!
//! Asserts the expected rule coverage across files, that the negative
//! control (`billing/rates.py`) stays clean, and that the whole scan is
//! deterministic. Read-only; no LLM, no executor.

use std::collections::BTreeSet;
use std::path::PathBuf;

use deterministic_ai_kernel::analyzer::ingestion::scan_workspace;
use deterministic_ai_kernel::analyzer::scan_primitives::run_static_scan;
use deterministic_ai_kernel::analyzer::triage::triage;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_billing_service")
}

fn findings() -> Vec<deterministic_ai_kernel::analyzer::triage::Finding> {
    let root = fixture();
    let inv = scan_workspace(&root).unwrap();
    triage(&run_static_scan(&inv, &root), &inv)
}

#[test]
fn expected_rule_coverage_across_files() {
    let fs = findings();
    let rules: BTreeSet<String> = fs.iter().map(|f| f.rule_id.clone()).collect();
    for expected in [
        "money-truncation",
        "floor-div-money",
        "offbyone-range",
        "float-equality",
        "none-arith",
        "todo-marker",
    ] {
        assert!(
            rules.contains(expected),
            "missing rule {expected}: {rules:?}"
        );
    }
}

#[test]
fn negative_control_rates_stays_clean() {
    let fs = findings();
    assert!(
        !fs.iter()
            .any(|f| f.evidence_locations[0].file == "billing/rates.py"),
        "billing/rates.py must produce no findings"
    );
}

#[test]
fn new_rules_fire_on_their_seeded_files() {
    let fs = findings();
    assert!(fs.iter().any(|f| {
        f.rule_id == "float-equality" && f.evidence_locations[0].file == "billing/settlement.py"
    }));
    assert!(fs.iter().any(|f| {
        f.rule_id == "floor-div-money" && f.evidence_locations[0].file == "billing/charges.py"
    }));
}

#[test]
fn scan_is_deterministic_and_findings_count_stable() {
    let a = findings();
    let b = findings();
    assert_eq!(a, b, "findings must be byte-identical across runs");
    // 7 seeded findings: truncation, floor-div, offbyone, float-eq(settlement),
    // none-arith, float-eq(test file, documented FP), todo-marker(limits).
    assert_eq!(a.len(), 7, "unexpected finding count: {a:?}");
}

#[test]
fn truncation_finding_is_remediation_candidate_with_repro() {
    let fs = findings();
    let trunc = fs
        .iter()
        .find(|f| f.rule_id == "money-truncation")
        .expect("truncation finding present");
    assert_eq!(trunc.evidence_locations[0].file, "billing/charges.py");
    assert_eq!(trunc.evidence_locations[0].line_start, 11);
}
