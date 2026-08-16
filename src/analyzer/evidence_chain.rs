//! Evidence chain verifier (v0.4-pilot-ops) — independent verification
//! of the remediation evidence trail.
//!
//! Trust model:
//! - EVERY hash is recomputed from bytes; nothing is taken from report
//!   prose or executor self-descriptions.
//! - Executor-produced files are UNTRUSTED INPUT: the test report is
//!   validated structurally against the real `test_report_v1` schema
//!   (version, classification, passed, exit_code, timed_out, argv) —
//!   never "believed".
//! - The verifier speaks only about CHAIN CONSISTENCY. It cannot and
//!   does not declare a fix correct: correctness is established by the
//!   real tests and human review.
//!
//! Overall statuses (exhaustive):
//! - `chain_inconsistent`                    — at least one link mismatch;
//! - `chain_incomplete`                      — no mismatches, but a
//!   mandatory link is missing;
//! - `chain_consistent_no_change`            — everything consistent, but
//!   no target file changed (honest: the executor may reject or no-op);
//! - `chain_consistent_remediation_evidenced`— consistent, files changed,
//!   structural test-pass evidence present.

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::operational::OperationalStamp;
use super::pilot_package::{blake3_hex, snapshot_hash_for, EvidencePackage};
use super::review_gate::{verify_decision_against_package, DecisionCore, ReviewDecision};

pub const CHAIN_SCHEMA_VERSION: &str = "evidence_chain_v1";

pub const LINK_CONTRACT: &str = "contract_integrity";
pub const LINK_PRE_STATE: &str = "pre_state_matches_snapshot";
pub const LINK_POST_STATE: &str = "post_state_present_and_changed";
pub const LINK_TEST_REPORT: &str = "test_report_passed";
pub const LINK_EVENT_LOG: &str = "event_log_present";

pub const STATUS_VERIFIED: &str = "verified";
pub const STATUS_MISMATCH: &str = "mismatch";
pub const STATUS_MISSING: &str = "missing";

pub const OVERALL_REMEDIATION_EVIDENCED: &str = "chain_consistent_remediation_evidenced";
pub const OVERALL_NO_CHANGE: &str = "chain_consistent_no_change";
pub const OVERALL_INCONSISTENT: &str = "chain_inconsistent";
pub const OVERALL_INCOMPLETE: &str = "chain_incomplete";

pub const CHAIN_DISCLAIMER: &str = "Chain consistency is NOT proof that the remediation is correct. \
Correctness is established by the real tests and human review; this verifier only cross-checks hashes and structure.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainLink {
    pub name: String,
    /// verified | mismatch | missing
    pub status: String,
    pub expected: String,
    pub actual: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainTargetState {
    pub path: String,
    pub snapshot_blake3: String,
    pub pre_blake3: Option<String>,
    pub post_blake3: Option<String>,
    /// None when either side is absent.
    pub changed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainInputs {
    pub package_manifest_blake3: String,
    pub decision_id: String,
    pub contract_blake3: String,
    pub test_report_blake3: Option<String>,
    pub event_log_blake3: Option<String>,
    pub targets: Vec<ChainTargetState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainCore {
    pub overall: String,
    pub links: Vec<ChainLink>,
    pub inputs: ChainInputs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceChainReport {
    pub schema_version: String,
    /// Content hash of `core`.
    pub chain_id: String,
    pub core: ChainCore,
    /// Operational envelope: the only timestamp-bearing part.
    pub operational: OperationalStamp,
}

fn link(name: &str, status: &str, expected: &str, actual: &str, detail: &str) -> ChainLink {
    ChainLink {
        name: name.to_string(),
        status: status.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
        detail: detail.to_string(),
    }
}

fn read_file_blake3(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(blake3::hash(&bytes).to_hex().to_string())
}

/// Structural validation of a `test_report_v1` document (schema taken
/// from the executor's TestReportV1). Returns Ok(()) only when the
/// report evidences a real passing run; every violation is reported.
fn validate_test_report(bytes: &str) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(bytes).map_err(|e| format!("report is not valid JSON: {e}"))?;
    let get = |field: &str| value.get(field);

    let version = get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "field 'version' missing".to_string())?;
    if version != "test_report_v1" {
        return Err(format!("unexpected report version '{version}'"));
    }
    let classification = get("classification")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "field 'classification' missing".to_string())?;
    if classification != "tests_passed" {
        return Err(format!(
            "classification is '{classification}', not tests_passed"
        ));
    }
    let passed = get("passed")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "field 'passed' missing".to_string())?;
    if !passed {
        return Err("'passed' is false while classification claims tests_passed".to_string());
    }
    let exit_code = get("exit_code")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "field 'exit_code' missing".to_string())?;
    if exit_code != 0 {
        return Err(format!("exit_code is {exit_code}, expected 0"));
    }
    let timed_out = get("timed_out")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "field 'timed_out' missing".to_string())?;
    if timed_out {
        return Err("report is marked timed_out".to_string());
    }
    let argv = get("argv")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "field 'argv' missing".to_string())?;
    if argv.is_empty() {
        return Err("argv is empty — no command was evidenced".to_string());
    }
    Ok(())
}

/// Verify the whole evidence chain. Read-only over every input.
#[allow(clippy::too_many_arguments)]
pub fn verify_chain(
    pkg: &EvidencePackage,
    decision: &ReviewDecision,
    pre_dir: &Path,
    post_dir: &Path,
    test_report: &Path,
    event_log: Option<&Path>,
) -> Result<ChainCore, String> {
    let core_decision: &DecisionCore = &decision.decision_core;
    let mut links: Vec<ChainLink> = Vec::new();

    // 1) Contract integrity: decision vs CURRENT package state.
    let contract_link = match verify_decision_against_package(pkg, core_decision) {
        Ok(()) => link(
            LINK_CONTRACT,
            STATUS_VERIFIED,
            &core_decision.contract_blake3,
            &core_decision.contract_blake3,
            "decision matches the current package (manifest, snapshot, contract)",
        ),
        Err(e) => link(
            LINK_CONTRACT,
            STATUS_MISMATCH,
            &core_decision.contract_blake3,
            "(see detail)",
            &e,
        ),
    };
    links.push(contract_link);

    // Target files from the contract.
    let task = pkg
        .tasks
        .iter()
        .find(|t| t.contract.finding.id == core_decision.finding_id)
        .ok_or_else(|| {
            format!(
                "contract for finding {} missing from package",
                core_decision.finding_id
            )
        })?;

    let mut targets = Vec::new();
    let mut pre_missing = false;
    let mut pre_mismatch = false;
    let mut post_missing = false;
    let mut any_changed = false;
    for rel in &task.contract.target_files {
        let snapshot = snapshot_hash_for(pkg, rel)
            .ok_or_else(|| format!("target {rel} missing from the package snapshot inventory"))?;
        let pre_hash = read_file_blake3(&pre_dir.join(rel));
        let post_hash = read_file_blake3(&post_dir.join(rel));
        if pre_hash.is_none() {
            pre_missing = true;
        } else if pre_hash.as_deref() != Some(snapshot.as_str()) {
            pre_mismatch = true;
        }
        if post_hash.is_none() {
            post_missing = true;
        }
        let changed = match (&pre_hash, &post_hash) {
            (Some(a), Some(b)) => {
                if a != b {
                    any_changed = true;
                }
                Some(a != b)
            }
            _ => None,
        };
        targets.push(ChainTargetState {
            path: rel.clone(),
            snapshot_blake3: snapshot,
            pre_blake3: pre_hash,
            post_blake3: post_hash,
            changed,
        });
    }

    // 2) Pre-state vs snapshot.
    let pre_link = if pre_mismatch {
        let detail = targets
            .iter()
            .filter(|t| t.pre_blake3.as_deref() != Some(t.snapshot_blake3.as_str()))
            .map(|t| t.path.clone())
            .collect::<Vec<_>>()
            .join(", ");
        link(
            LINK_PRE_STATE,
            STATUS_MISMATCH,
            "pre-state hashes equal to snapshot",
            "drift detected",
            &format!("pre-state drift in: {detail}"),
        )
    } else if pre_missing {
        link(
            LINK_PRE_STATE,
            STATUS_MISSING,
            "all target files present in pre-dir",
            "some target files absent",
            "pre-dir lacks one or more target files",
        )
    } else {
        link(
            LINK_PRE_STATE,
            STATUS_VERIFIED,
            "pre-state hashes equal to snapshot",
            "equal",
            &format!("{} target file(s) match the snapshot", targets.len()),
        )
    };
    links.push(pre_link);

    // 3) Post-state presence + change detection.
    let post_link = if post_missing {
        link(
            LINK_POST_STATE,
            STATUS_MISSING,
            "all target files present in post-dir",
            "some target files absent",
            "post-dir lacks one or more target files",
        )
    } else if any_changed {
        link(
            LINK_POST_STATE,
            STATUS_VERIFIED,
            "post-state present",
            "present, changed",
            "at least one target file differs from pre-state",
        )
    } else {
        link(
            LINK_POST_STATE,
            STATUS_VERIFIED,
            "post-state present",
            "present, unchanged",
            "no target file changed — executor may have rejected or no-op'ed",
        )
    };
    links.push(post_link);

    // 4) Test report: structural validation only, hashes recomputed.
    let mut test_report_blake3 = None;
    let report_link = match std::fs::read_to_string(test_report) {
        Ok(bytes) => {
            test_report_blake3 = Some(blake3_hex(&bytes));
            match validate_test_report(&bytes) {
                Ok(()) => link(
                    LINK_TEST_REPORT,
                    STATUS_VERIFIED,
                    "test_report_v1: tests_passed, passed, exit_code 0, not timed out, argv non-empty",
                    "matches",
                    "structural validation against the executor's TestReportV1 schema",
                ),
                Err(e) => link(
                    LINK_TEST_REPORT,
                    STATUS_MISMATCH,
                    "test_report_v1 with tests_passed semantics",
                    "validation failed",
                    &e,
                ),
            }
        }
        Err(e) => link(
            LINK_TEST_REPORT,
            STATUS_MISSING,
            "readable test_report_v1 file",
            "absent or unreadable",
            &e.to_string(),
        ),
    };
    links.push(report_link);

    // 5) Event log (optional): existence + JSON parsability only.
    let mut event_log_blake3 = None;
    if let Some(log_path) = event_log {
        let log_link = match std::fs::read_to_string(log_path) {
            Ok(bytes) => match serde_json::from_str::<serde_json::Value>(&bytes) {
                Ok(_) => {
                    event_log_blake3 = Some(blake3_hex(&bytes));
                    link(
                        LINK_EVENT_LOG,
                        STATUS_VERIFIED,
                        "parseable JSON event log",
                        "parseable",
                        "presence only — contents are not interpreted",
                    )
                }
                Err(e) => link(
                    LINK_EVENT_LOG,
                    STATUS_MISMATCH,
                    "parseable JSON",
                    "malformed",
                    &e.to_string(),
                ),
            },
            Err(e) => link(
                LINK_EVENT_LOG,
                STATUS_MISSING,
                "readable event log",
                "absent or unreadable",
                &e.to_string(),
            ),
        };
        links.push(log_link);
    }

    // Overall status (rules in priority order).
    let has_mismatch = links.iter().any(|l| l.status == STATUS_MISMATCH);
    let mandatory_missing = links.iter().any(|l| {
        l.status == STATUS_MISSING
            && matches!(
                l.name.as_str(),
                // contract integrity cannot be "missing"; these are the
                // mandatory evidence links:
                "pre_state_matches_snapshot"
                    | "post_state_present_and_changed"
                    | "test_report_passed"
            )
    });
    let overall = if has_mismatch {
        OVERALL_INCONSISTENT
    } else if mandatory_missing {
        OVERALL_INCOMPLETE
    } else if !any_changed {
        OVERALL_NO_CHANGE
    } else {
        OVERALL_REMEDIATION_EVIDENCED
    };

    Ok(ChainCore {
        overall: overall.to_string(),
        links,
        inputs: ChainInputs {
            package_manifest_blake3: core_decision.package_manifest_blake3.clone(),
            decision_id: decision.decision_id.clone(),
            contract_blake3: core_decision.contract_blake3.clone(),
            test_report_blake3,
            event_log_blake3,
            targets,
        },
    })
}

pub fn chain_id_for(core: &ChainCore) -> String {
    let json = serde_json::to_string(core).expect("chain core serialization");
    blake3_hex(&json)
}

/// Deterministic markdown rendering of the chain report.
pub fn render_chain_md(report: &EvidenceChainReport) -> String {
    let core = &report.core;
    let mut md = format!(
        "# Chain of Custody — Evidence Verification

- Chain id (content hash): `{id}`
- Decision id: `{did}`
- Contract BLAKE3: `{chash}`
- Overall status: **{overall}**

{disclaimer}

| Link | Status | Detail |
|---|---|---|
",
        id = report.chain_id,
        did = core.inputs.decision_id,
        chash = core.inputs.contract_blake3,
        overall = core.overall,
        disclaimer = CHAIN_DISCLAIMER
    );
    for l in &core.links {
        md.push_str(&format!(
            "| {name} | {status} | {detail} |\n",
            name = l.name,
            status = l.status,
            detail = l.detail
        ));
    }
    md.push_str("\n## Target files (recomputed hashes)\n\n| Path | Snapshot | Pre | Post | Changed |\n|---|---|---|---|---|\n");
    for t in &core.inputs.targets {
        md.push_str(&format!(
            "| `{path}` | `{snap}` | {pre} | {post} | {changed} |\n",
            path = t.path,
            snap = t.snapshot_blake3,
            pre = t.pre_blake3.as_deref().unwrap_or("(missing)"),
            post = t.post_blake3.as_deref().unwrap_or("(missing)"),
            changed = match t.changed {
                Some(true) => "yes",
                Some(false) => "no",
                None => "(unknown)",
            }
        ));
    }
    md.push_str(
        "\nAll hashes above were recomputed by this verifier from file bytes. \
No executor self-reported status was accepted without structural validation.\n",
    );
    md
}

/// Verify and write `evidence_chain_v1.json` + `CHAIN_OF_CUSTODY.md`.
pub fn write_chain_report(
    pkg: &EvidencePackage,
    decision: &ReviewDecision,
    pre_dir: &Path,
    post_dir: &Path,
    test_report: &Path,
    event_log: Option<&Path>,
    out_dir: &Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf, EvidenceChainReport), String> {
    let core = verify_chain(pkg, decision, pre_dir, post_dir, test_report, event_log)?;
    let report = EvidenceChainReport {
        schema_version: CHAIN_SCHEMA_VERSION.to_string(),
        chain_id: chain_id_for(&core),
        core,
        operational: super::operational::now_stamp(),
    };
    std::fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let json_path = out_dir.join("evidence_chain_v1.json");
    let md_path = out_dir.join("CHAIN_OF_CUSTODY.md");
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&report).expect("chain serialization"),
    )
    .map_err(|e| format!("write {}: {e}", json_path.display()))?;
    std::fs::write(&md_path, render_chain_md(&report))
        .map_err(|e| format!("write {}: {e}", md_path.display()))?;
    Ok((json_path, md_path, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::pilot_package::load_evidence_package;
    use crate::analyzer::pilot_report::{build_bundle, write_bundle_artifacts};
    use crate::analyzer::review_gate::{record_review, ReviewInput, DECISION_APPROVE};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const FINDING: &str = "MONEY-TRUNCATION-LEDGER-15";

    struct ChainRig {
        _pkg_dir: tempfile::TempDir,
        _work_dir: tempfile::TempDir,
        pkg: EvidencePackage,
        decision: ReviewDecision,
        pre_dir: PathBuf,
        post_dir: PathBuf,
        report_path: PathBuf,
    }

    fn passing_report() -> String {
        r#"{
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
  "workspace": "/isolated/pilot_fintech_copy",
  "captured_unix": 1786850000
}"#
        .to_string()
    }

    fn rig() -> ChainRig {
        let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_fintech");
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
            rationale: "approved for isolated attempt".to_string(),
            repro_test_path: Some("test_ledger.py".to_string()),
            override_candidate_only: false,
        };
        let (_, decision) = record_review(&pkg, &input, pkg_dir.path()).unwrap();

        // Pre/post dirs from the fixture (pre == pristine copy).
        let sim = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("analyzer_examples/simulated_executor_evidence");
        let work = tempfile::tempdir().unwrap();
        let pre_dir = work.path().join("pre");
        let post_dir = work.path().join("post");
        copy_tree(&sim.join("pre"), &pre_dir);
        copy_tree(&sim.join("post"), &post_dir);
        let report_path = work.path().join("test_report_v1.json");
        std::fs::write(&report_path, passing_report()).unwrap();
        ChainRig {
            _pkg_dir: pkg_dir,
            _work_dir: work,
            pkg,
            decision,
            pre_dir,
            post_dir,
            report_path,
        }
    }

    fn copy_tree(from: &Path, to: &Path) {
        for entry in walkdir(from) {
            let rel = entry.strip_prefix(from).unwrap();
            let dst = to.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&dst).unwrap();
            } else {
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::copy(&entry, &dst).unwrap();
            }
        }
    }

    fn walkdir(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
        out
    }

    #[test]
    fn happy_path_yields_remediation_evidenced() {
        let rig = rig();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_REMEDIATION_EVIDENCED, "{core:?}");
        assert!(core.links.iter().all(|l| l.status == STATUS_VERIFIED));
        assert!(core.inputs.targets[0].changed == Some(true));
    }

    #[test]
    fn identical_pre_post_yields_no_change() {
        let rig = rig();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.pre_dir, // post == pre
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_NO_CHANGE);
    }

    #[test]
    fn failing_report_yields_inconsistent() {
        let rig = rig();
        let failing = r#"{
  "version": "test_report_v1",
  "command_id": "python_test_file",
  "argv": ["python3", "-", "test_ledger.py"],
  "exit_code": 1,
  "passed": false,
  "timed_out": false,
  "classification": "tests_failed",
  "stdout_tail": "",
  "stderr_tail": "kernel test harness: FAILED test_post_amount_keeps_sub_cent_fraction",
  "duration_ms": 42,
  "workspace": "/isolated/pilot_fintech_copy",
  "captured_unix": 1786850000
}"#;
        std::fs::write(&rig.report_path, failing).unwrap();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
        let report_link = core
            .links
            .iter()
            .find(|l| l.name == LINK_TEST_REPORT)
            .unwrap();
        assert_eq!(report_link.status, STATUS_MISMATCH);
    }

    #[test]
    fn missing_report_yields_incomplete() {
        let rig = rig();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path.join("no_such_file.json"),
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_INCOMPLETE);
    }

    #[test]
    fn malformed_report_yields_inconsistent() {
        let rig = rig();
        std::fs::write(&rig.report_path, "{ not json").unwrap();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
    }

    #[test]
    fn pre_state_drift_yields_inconsistent() {
        let rig = rig();
        let drifted = rig.pre_dir.join("billing/ledger.py");
        std::fs::write(
            &drifted,
            std::fs::read_to_string(&drifted).unwrap() + "# drift\n",
        )
        .unwrap();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
        let pre_link = core
            .links
            .iter()
            .find(|l| l.name == LINK_PRE_STATE)
            .unwrap();
        assert_eq!(pre_link.status, STATUS_MISMATCH);
    }

    #[test]
    fn tampered_decision_yields_contract_mismatch() {
        let rig = rig();
        let mut decision = rig.decision.clone();
        decision.decision_core.contract_blake3 = "0".repeat(64);
        let core = verify_chain(
            &rig.pkg,
            &decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
        let link = core.links.iter().find(|l| l.name == LINK_CONTRACT).unwrap();
        assert_eq!(link.status, STATUS_MISMATCH);
    }

    #[test]
    fn event_log_is_optional_but_validated_when_present() {
        let rig = rig();
        // Absent → still evidenced (link simply not added when None).
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_REMEDIATION_EVIDENCED);
        assert!(core.links.iter().all(|l| l.name != LINK_EVENT_LOG));

        // Present + parseable → verified.
        let log = rig.pre_dir.join("event_log.json");
        std::fs::write(&log, "[{\"event\":\"step_committed\"}]").unwrap();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            Some(&log),
        )
        .unwrap();
        let link = core
            .links
            .iter()
            .find(|l| l.name == LINK_EVENT_LOG)
            .unwrap();
        assert_eq!(link.status, STATUS_VERIFIED);
        assert!(core.inputs.event_log_blake3.is_some());

        // Malformed → mismatch → inconsistent.
        std::fs::write(&log, "not-json").unwrap();
        let core = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            Some(&log),
        )
        .unwrap();
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
    }

    #[test]
    fn chain_core_is_byte_stable_and_timestamp_free() {
        let rig = rig();
        let a = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        let b = verify_chain(
            &rig.pkg,
            &rig.decision,
            &rig.pre_dir,
            &rig.post_dir,
            &rig.report_path,
            None,
        )
        .unwrap();
        assert_eq!(chain_id_for(&a), chain_id_for(&b));
        let json = serde_json::to_string(&a).unwrap();
        for banned in ["recorded_at", "ts_unix"] {
            assert!(!json.contains(banned), "core must be timestamp-free");
        }
        let md = render_chain_md(&EvidenceChainReport {
            schema_version: CHAIN_SCHEMA_VERSION.to_string(),
            chain_id: chain_id_for(&a),
            core: a.clone(),
            operational: super::super::operational::now_stamp(),
        });
        assert!(md.contains(CHAIN_DISCLAIMER));
        for banned in crate::analyzer::pilot_report::FORBIDDEN_CLAIMS {
            assert!(
                !md.to_lowercase().contains(&banned.to_lowercase()),
                "forbidden claim: {banned}"
            );
        }
    }
}
