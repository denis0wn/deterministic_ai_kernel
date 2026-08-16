//! Integration tests for the v0.4-pilot-ops boundary:
//! package → review decision → work order → chain verification.
//!
//! The executor is NOT run anywhere in these tests: the executor-side
//! evidence comes from analyzer_examples/simulated_executor_evidence,
//! which is explicitly labeled SIMULATED. No LLM, no MLX.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use deterministic_ai_kernel::analyzer::evidence_chain::{
    chain_id_for, verify_chain, OVERALL_INCOMPLETE, OVERALL_REMEDIATION_EVIDENCED, STATUS_VERIFIED,
};
use deterministic_ai_kernel::analyzer::pilot_package::load_evidence_package;
use deterministic_ai_kernel::analyzer::pilot_report::{build_bundle, write_bundle_artifacts};
use deterministic_ai_kernel::analyzer::review_gate::{
    record_review, ReviewInput, DECISION_APPROVE,
};
use deterministic_ai_kernel::analyzer::work_order::{build_work_order, WORK_ORDER_DISCLAIMER};

const FINDING: &str = "MONEY-TRUNCATION-LEDGER-15";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bin(name: &str) -> PathBuf {
    repo_root().join(format!("target/debug/{name}"))
}

fn simulated_dir() -> PathBuf {
    repo_root().join("analyzer_examples/simulated_executor_evidence")
}

/// BLAKE3 of every file under `root` (deterministic map).
fn fingerprint(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
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
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&p).unwrap();
            out.insert(rel, blake3::hash(&bytes).to_hex().to_string());
        }
    }
    out
}

fn copy_tree(from: &Path, to: &Path) {
    for (rel, _) in fingerprint(from) {
        let src = from.join(&rel);
        let dst = to.join(&rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::copy(&src, &dst).unwrap();
    }
}

/// Evidence package in a temp dir, with an approved decision recorded.
fn pilot_flow_rig() -> (tempfile::TempDir, PathBuf) {
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
        reviewer: "pilot-operator".to_string(),
        rationale: "integration test approval".to_string(),
        repro_test_path: Some("test_ledger.py".to_string()),
        override_candidate_only: false,
    };
    record_review(&pkg, &input, pkg_dir.path()).unwrap();
    let decision_path = pkg_dir
        .path()
        .join(format!("review_decisions/{FINDING}.json"));
    (pkg_dir, decision_path)
}

/// Isolated pre/post copies + the simulated passing test report.
fn executor_evidence_rig(tmp: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let sim = simulated_dir();
    let pre = tmp.join("pre_copy");
    let post = tmp.join("post_copy");
    copy_tree(&sim.join("pre"), &pre);
    copy_tree(&sim.join("post"), &post);
    let report = tmp.join("test_report_v1.json");
    std::fs::copy(sim.join("test_report_v1.json"), &report).unwrap();
    (pre, post, report)
}

#[test]
fn full_flow_yields_evidenced_chain_and_is_byte_stable() {
    let (pkg_dir, decision_path) = pilot_flow_rig();
    let pkg = load_evidence_package(pkg_dir.path()).unwrap();
    let decision =
        deterministic_ai_kernel::analyzer::review_gate::load_review_decision(&decision_path)
            .unwrap();

    // Work order builds on the approved decision and stays passive.
    let order = build_work_order(&pkg, &decision).unwrap();
    assert_eq!(order.core.finding_id, FINDING);
    assert!(order
        .operator_instructions
        .iter()
        .any(|s| s.contains("ISOLATED")));
    let md = deterministic_ai_kernel::analyzer::work_order::render_work_order_md(&order);
    assert!(md.contains(WORK_ORDER_DISCLAIMER));

    // Evidence chain on the SIMULATED executor evidence.
    let evidence_tmp = tempfile::tempdir().unwrap();
    let (pre, post, report) = executor_evidence_rig(evidence_tmp.path());

    let fp_pkg_before = fingerprint(pkg_dir.path());
    let fp_pre_before = fingerprint(&pre);
    let fp_post_before = fingerprint(&post);

    let core1 = verify_chain(&pkg, &decision, &pre, &post, &report, None).unwrap();
    let core2 = verify_chain(&pkg, &decision, &pre, &post, &report, None).unwrap();
    assert_eq!(core1.overall, OVERALL_REMEDIATION_EVIDENCED, "{core1:?}");
    assert!(core1.links.iter().all(|l| l.status == STATUS_VERIFIED));
    // Byte-stable cores (timestamp-free).
    assert_eq!(chain_id_for(&core1), chain_id_for(&core2));
    assert_eq!(
        serde_json::to_string(&core1).unwrap(),
        serde_json::to_string(&core2).unwrap()
    );

    // Optional event log link verified when present.
    let log = simulated_dir().join("event_log_sample.json");
    let core_log = verify_chain(&pkg, &decision, &pre, &post, &report, Some(&log)).unwrap();
    assert_eq!(core_log.overall, OVERALL_REMEDIATION_EVIDENCED);
    assert!(core_log.links.len() == core1.links.len() + 1);

    // Read-only: verification touched nothing outside its own output.
    assert_eq!(fp_pkg_before, fingerprint(pkg_dir.path()));
    assert_eq!(fp_pre_before, fingerprint(&pre));
    assert_eq!(fp_post_before, fingerprint(&post));
}

#[test]
fn missing_test_report_yields_incomplete_chain() {
    let (pkg_dir, decision_path) = pilot_flow_rig();
    let pkg = load_evidence_package(pkg_dir.path()).unwrap();
    let decision =
        deterministic_ai_kernel::analyzer::review_gate::load_review_decision(&decision_path)
            .unwrap();
    let evidence_tmp = tempfile::tempdir().unwrap();
    let (pre, post, report) = executor_evidence_rig(evidence_tmp.path());
    std::fs::remove_file(&report).unwrap();

    let core = verify_chain(&pkg, &decision, &pre, &post, &report, None).unwrap();
    assert_eq!(core.overall, OVERALL_INCOMPLETE);
}

#[test]
fn cli_review_work_order_chain_happy_path() {
    let (pkg_dir, _decision_path) = pilot_flow_rig();
    let decision_file = pkg_dir
        .path()
        .join(format!("review_decisions/{FINDING}.json"));
    assert!(decision_file.exists(), "review CLI artifact expected");

    // Work order via CLI.
    let out_wo = pkg_dir.path().join("wo_out");
    let status = std::process::Command::new(bin("analyzer_work_order"))
        .arg("--package")
        .arg(pkg_dir.path())
        .arg("--decision")
        .arg(&decision_file)
        .arg("--out")
        .arg(&out_wo)
        .status()
        .expect("spawn analyzer_work_order");
    assert!(status.success());
    assert!(out_wo.join("work_order_v1.json").exists());
    assert!(out_wo.join("WORK_ORDER.md").exists());

    // Chain via CLI on the simulated evidence.
    let evidence_tmp = tempfile::tempdir().unwrap();
    let (pre, post, report) = executor_evidence_rig(evidence_tmp.path());
    let out_chain = evidence_tmp.path().join("chain_out");
    let output = std::process::Command::new(bin("analyzer_chain_verify"))
        .arg("--package")
        .arg(pkg_dir.path())
        .arg("--decision")
        .arg(&decision_file)
        .arg("--pre-dir")
        .arg(&pre)
        .arg("--post-dir")
        .arg(&post)
        .arg("--test-report")
        .arg(&report)
        .arg("--out")
        .arg(&out_chain)
        .output()
        .expect("spawn analyzer_chain_verify");
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("chain_consistent_remediation_evidenced"),
        "{stdout}"
    );
    assert!(out_chain.join("evidence_chain_v1.json").exists());
    assert!(out_chain.join("CHAIN_OF_CUSTODY.md").exists());
}

#[test]
fn cli_review_refuses_tampered_package() {
    let (pkg_dir, _) = pilot_flow_rig();
    // Tamper with the findings artifact after the package was produced:
    // corrupt the first evidence snapshot hash so it no longer matches
    // the manifest inventory.
    let findings_path = pkg_dir.path().join("findings_v1.json");
    let mut content = std::fs::read_to_string(&findings_path).unwrap();
    let first = content.find("\"file_blake3\": \"").unwrap() + 16;
    let mut h = content[first..first + 64].to_string();
    h.replace_range(0..1, if h.starts_with('0') { "1" } else { "0" });
    content.replace_range(first..first + 64, &h);
    std::fs::write(&findings_path, content).unwrap();

    let status = std::process::Command::new(bin("analyzer_review"))
        .arg("--package")
        .arg(pkg_dir.path())
        .arg("--finding")
        .arg(FINDING)
        .arg("--decision")
        .arg("approve")
        .arg("--reviewer")
        .arg("attacker")
        .arg("--rationale")
        .arg("attempt after tamper")
        .arg("--out")
        .arg(pkg_dir.path().join("out2"))
        .status()
        .expect("spawn analyzer_review");
    assert!(
        !status.success(),
        "tampered package must be refused fail-closed"
    );
}

#[test]
fn cli_chain_reports_incomplete_without_test_report() {
    let (pkg_dir, _) = pilot_flow_rig();
    let decision_file = pkg_dir
        .path()
        .join(format!("review_decisions/{FINDING}.json"));
    let evidence_tmp = tempfile::tempdir().unwrap();
    let (pre, post, report) = executor_evidence_rig(evidence_tmp.path());
    std::fs::remove_file(&report).unwrap();

    let output = std::process::Command::new(bin("analyzer_chain_verify"))
        .arg("--package")
        .arg(pkg_dir.path())
        .arg("--decision")
        .arg(&decision_file)
        .arg("--pre-dir")
        .arg(&pre)
        .arg("--post-dir")
        .arg(&post)
        .arg("--test-report")
        .arg(&report)
        .arg("--out")
        .arg(evidence_tmp.path().join("chain_out"))
        .output()
        .expect("spawn analyzer_chain_verify");
    // An incomplete chain is an honest RESULT, not a tool error.
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("chain_incomplete"), "{stdout}");
}
