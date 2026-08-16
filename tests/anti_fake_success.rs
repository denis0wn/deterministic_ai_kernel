//! Anti-fake-success regression battery (v0.4-pilot-ops negative
//! acceptance phase).
//!
//! Invariant under test: the evidence chain can reach
//! `chain_consistent_remediation_evidenced` ONLY through a structurally
//! valid, passing `test_report_v1`. Every forged variant — exit-code
//! lies, passed-flag lies, classification lies, empty argv, wrong
//! version, timeout, missing fields — must be caught and must never be
//! promotable to a success status. No model, no executor: this guards
//! the VERIFIER itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deterministic_ai_kernel::analyzer::evidence_chain::{
    verify_chain, LINK_TEST_REPORT, OVERALL_INCONSISTENT, OVERALL_REMEDIATION_EVIDENCED,
    STATUS_MISMATCH,
};
use deterministic_ai_kernel::analyzer::pilot_package::load_evidence_package;
use deterministic_ai_kernel::analyzer::pilot_report::{build_bundle, write_bundle_artifacts};
use deterministic_ai_kernel::analyzer::review_gate::{
    load_review_decision, record_review, ReviewInput, DECISION_APPROVE,
};

const FINDING: &str = "MONEY-TRUNCATION-LEDGER-15";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn copy_tree(from: &Path, to: &Path) {
    let mut stack = vec![from.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(from).unwrap();
            let dst = to.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::copy(&p, &dst).unwrap();
        }
    }
}

/// Package + approved decision + pre/post copies of the SIMULATED
/// executor evidence (clearly labeled as simulated upstream).
struct ForgeryRig {
    _pkg_dir: tempfile::TempDir,
    _ev_dir: tempfile::TempDir,
    pkg_dir: PathBuf,
    pre: PathBuf,
    post: PathBuf,
    report: PathBuf,
}

fn rig() -> ForgeryRig {
    let ws = repo_root().join("analyzer_examples/pilot_fintech");
    let mut repro = BTreeMap::new();
    repro.insert(FINDING.to_string(), "test_ledger.py".to_string());
    let bundle = build_bundle(&ws, &repro, &[]).unwrap();
    let pkg_dir = tempfile::tempdir().unwrap();
    write_bundle_artifacts(&bundle, pkg_dir.path()).unwrap();
    let pkg = load_evidence_package(pkg_dir.path()).unwrap();
    let input = ReviewInput {
        finding_id: FINDING.to_string(),
        decision: DECISION_APPROVE.to_string(),
        reviewer: "anti-fake-success-battery".to_string(),
        rationale: "test rig approval".to_string(),
        repro_test_path: Some("test_ledger.py".to_string()),
        override_candidate_only: false,
    };
    record_review(&pkg, &input, pkg_dir.path()).unwrap();

    let sim = repo_root().join("analyzer_examples/simulated_executor_evidence");
    let ev_dir = tempfile::tempdir().unwrap();
    let pre = ev_dir.path().join("pre");
    let post = ev_dir.path().join("post");
    copy_tree(&sim.join("pre"), &pre);
    copy_tree(&sim.join("post"), &post);
    let report = ev_dir.path().join("test_report_v1.json");
    std::fs::copy(sim.join("test_report_v1.json"), &report).unwrap();

    let pkg_path = pkg_dir.path().to_path_buf();
    ForgeryRig {
        _pkg_dir: pkg_dir,
        _ev_dir: ev_dir,
        pkg_dir: pkg_path,
        pre,
        post,
        report,
    }
}

fn chain_overall(rig: &ForgeryRig) -> (String, Vec<(String, String)>) {
    let pkg = load_evidence_package(&rig.pkg_dir).unwrap();
    let decision =
        load_review_decision(&rig.pkg_dir.join(format!("review_decisions/{FINDING}.json")))
            .unwrap();
    let core = verify_chain(&pkg, &decision, &rig.pre, &rig.post, &rig.report, None).unwrap();
    let links = core
        .links
        .iter()
        .map(|l| (l.name.clone(), l.status.clone()))
        .collect();
    (core.overall, links)
}

fn write_report(rig: &ForgeryRig, json: &str) {
    std::fs::write(&rig.report, json).unwrap();
}

/// A structurally valid passing report (real executor schema) — the ONLY
/// input allowed to reach the success status in this battery's positive
/// control.
const VALID_PASSING_REPORT: &str = r#"{
  "version": "test_report_v1",
  "command_id": "python_test_file",
  "argv": ["python3", "-", "test_ledger.py"],
  "exit_code": 0,
  "passed": true,
  "timed_out": false,
  "classification": "tests_passed",
  "stdout_tail": "kernel test harness: OK",
  "stderr_tail": "",
  "duration_ms": 42,
  "workspace": "/isolated/copy",
  "captured_unix": 1786850000
}"#;

#[test]
fn positive_control_only_valid_report_reaches_evidenced() {
    let rig = rig();
    write_report(&rig, VALID_PASSING_REPORT);
    let (overall, _) = chain_overall(&rig);
    assert_eq!(overall, OVERALL_REMEDIATION_EVIDENCED);
}

#[test]
fn forged_reports_can_never_reach_remediation_evidenced() {
    // Each variant lies about success in a different way.
    let forgeries: [(&str, String); 7] = [
        (
            "exit_code_lies",
            VALID_PASSING_REPORT.replace("\"exit_code\": 0", "\"exit_code\": 1"),
        ),
        (
            "passed_flag_lies",
            VALID_PASSING_REPORT.replace("\"passed\": true", "\"passed\": false"),
        ),
        (
            "classification_lies",
            VALID_PASSING_REPORT.replace(
                "\"classification\": \"tests_passed\"",
                "\"classification\": \"tests_failed\"",
            ),
        ),
        (
            "empty_argv",
            VALID_PASSING_REPORT.replace(
                "\"argv\": [\"python3\", \"-\", \"test_ledger.py\"]",
                "\"argv\": []",
            ),
        ),
        (
            "wrong_version",
            VALID_PASSING_REPORT.replace(
                "\"version\": \"test_report_v1\"",
                "\"version\": \"test_report_v2\"",
            ),
        ),
        (
            "timed_out_but_passing_fields",
            VALID_PASSING_REPORT.replace("\"timed_out\": false", "\"timed_out\": true"),
        ),
        (
            "missing_classification",
            VALID_PASSING_REPORT.replace("  \"classification\": \"tests_passed\",\n", ""),
        ),
    ];

    for (name, forged) in forgeries {
        let rig = rig();
        write_report(&rig, &forged);
        let (overall, links) = chain_overall(&rig);
        assert_ne!(
            overall, OVERALL_REMEDIATION_EVIDENCED,
            "forgery '{name}' must never be promotable to success"
        );
        assert_eq!(
            overall, OVERALL_INCONSISTENT,
            "forgery '{name}' must be flagged inconsistent"
        );
        let report_link = links
            .iter()
            .find(|(n, _)| n == LINK_TEST_REPORT)
            .expect("test report link present");
        assert_eq!(
            report_link.1, STATUS_MISMATCH,
            "forgery '{name}' must mismatch at the test-report link"
        );
    }
}

#[test]
fn absent_report_is_incomplete_never_success() {
    let rig = rig();
    std::fs::remove_file(&rig.report).unwrap();
    let (overall, _) = chain_overall(&rig);
    assert_ne!(overall, OVERALL_REMEDIATION_EVIDENCED);
    assert_eq!(
        overall,
        deterministic_ai_kernel::analyzer::evidence_chain::OVERALL_INCOMPLETE,
        "missing evidence must stay missing — never promoted"
    );
}

#[test]
fn unchanged_workspace_is_no_change_never_success() {
    let rig = rig();
    write_report(&rig, VALID_PASSING_REPORT);
    // Post identical to pre: even a valid passing report cannot claim a
    // remediation that changed nothing.
    let pkg = load_evidence_package(&rig.pkg_dir).unwrap();
    let decision =
        load_review_decision(&rig.pkg_dir.join(format!("review_decisions/{FINDING}.json")))
            .unwrap();
    let core = verify_chain(&pkg, &decision, &rig.pre, &rig.pre, &rig.report, None).unwrap();
    assert_eq!(
        core.overall,
        deterministic_ai_kernel::analyzer::evidence_chain::OVERALL_NO_CHANGE
    );
}
