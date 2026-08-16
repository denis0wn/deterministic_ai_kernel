//! Review gate (v0.4-pilot-ops) — the human approval boundary as a
//! tamper-evident artifact.
//!
//! A finding crosses from "analyzer proposal" to "approved remediation
//! attempt" ONLY through a signed-off ReviewDecision recorded here. The
//! analyzer still performs no effects: this module validates the package
//! and records the human's choice, nothing more.
//!
//! Determinism: `decision_core` is byte-stable (no timestamps) and its
//! content hash is the `decision_id`. Wall-clock metadata lives only in
//! the attached `OperationalStamp`, which never participates in hashes.

use serde::{Deserialize, Serialize};

use super::operational::OperationalStamp;
use super::pilot_package::{
    blake3_hex, package_manifest_blake3, snapshot_hash_for, verify_finding_anchoring,
    EvidencePackage,
};
use super::task_emitter::READINESS_REMEDIATION_READY;

pub const DECISION_SCHEMA_VERSION: &str = "review_decision_v1";
pub const DECISION_APPROVE: &str = "approve";
pub const DECISION_REJECT: &str = "reject";
pub const DECISION_DEFER: &str = "defer";

/// Byte-stable, hashable core of a review decision (no time fields).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionCore {
    pub finding_id: String,
    /// `approve` | `reject` | `defer`.
    pub decision: String,
    /// BLAKE3 of the canonical TaskContract v0 JSON, recomputed from the
    /// package at decision time.
    pub contract_blake3: String,
    /// BLAKE3 of the package's evidence_manifest_v1.json bytes.
    pub package_manifest_blake3: String,
    /// Workspace snapshot hash recorded in that manifest.
    pub workspace_snapshot_blake3: String,
    /// True only when the operator explicitly overrode the
    /// `candidate_only` readiness policy; always false for reject/defer.
    pub override_candidate_only: bool,
    /// Operator-provided reproducible test (workspace-relative), when
    /// given.
    pub repro_test_path: Option<String>,
    /// Free text; no identity systems, no PII requirements.
    pub reviewer: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewDecision {
    pub schema_version: String,
    /// Content hash of `decision_core` (canonical JSON).
    pub decision_id: String,
    pub decision_core: DecisionCore,
    /// Operational envelope: the only place a timestamp exists.
    pub operational: OperationalStamp,
}

/// Operator input for one review.
#[derive(Debug, Clone)]
pub struct ReviewInput {
    pub finding_id: String,
    pub decision: String,
    pub reviewer: String,
    pub rationale: String,
    pub repro_test_path: Option<String>,
    pub override_candidate_only: bool,
}

pub fn decision_id_for(core: &DecisionCore) -> String {
    let json = serde_json::to_string(core).expect("decision core serialization");
    blake3_hex(&json)
}

/// Pure (clock-free) review assessment. All fail-closed checks live
/// here; `record_review` only adds the operational stamp and writes.
pub fn assess_review(pkg: &EvidencePackage, input: &ReviewInput) -> Result<DecisionCore, String> {
    let decision = input.decision.trim();
    if ![DECISION_APPROVE, DECISION_REJECT, DECISION_DEFER].contains(&decision) {
        return Err(format!(
            "invalid decision '{}' (expected approve|reject|defer)",
            input.decision
        ));
    }
    if input.reviewer.trim().is_empty() {
        return Err("reviewer must not be empty (human accountability)".to_string());
    }
    if input.rationale.trim().is_empty() {
        return Err("rationale must not be empty (audit trail)".to_string());
    }

    // Package integrity for this finding (tamper detection).
    verify_finding_anchoring(pkg, &input.finding_id)?;

    // The contract and its recomputed hash must exist.
    let task = pkg
        .tasks
        .iter()
        .find(|t| t.contract.finding.id == input.finding_id)
        .ok_or_else(|| format!("no task contract for finding {}", input.finding_id))?;
    let contract_blake3 = pkg
        .contract_hashes
        .get(&input.finding_id)
        .cloned()
        .ok_or_else(|| format!("contract hash missing for {}", input.finding_id))?;

    // Reproducible test, when supplied, must be part of the snapshot.
    if let Some(repro) = &input.repro_test_path {
        if snapshot_hash_for(pkg, repro).is_none() {
            return Err(format!(
                "repro test {repro} is not in the package snapshot inventory"
            ));
        }
    }

    // Approval policy: remediation_ready only, unless the operator
    // explicitly overrides (and the override is recorded forever).
    if decision == DECISION_APPROVE
        && task.readiness.readiness != READINESS_REMEDIATION_READY
        && !input.override_candidate_only
    {
        return Err(format!(
            "finding {} is '{}': approve requires remediation_ready (operator-provided reproducible test) or an explicit --override-candidate-only",
            input.finding_id, task.readiness.readiness
        ));
    }

    Ok(DecisionCore {
        finding_id: input.finding_id.clone(),
        decision: decision.to_string(),
        contract_blake3,
        package_manifest_blake3: package_manifest_blake3(pkg),
        workspace_snapshot_blake3: pkg.manifest.workspace_snapshot_blake3.clone(),
        override_candidate_only: decision == DECISION_APPROVE && input.override_candidate_only,
        repro_test_path: input.repro_test_path.clone(),
        reviewer: input.reviewer.trim().to_string(),
        rationale: input.rationale.trim().to_string(),
    })
}

/// Assess, stamp (operational only) and write the decision to
/// `<out>/review_decisions/<finding_id>.json`. Returns the path.
pub fn record_review(
    pkg: &EvidencePackage,
    input: &ReviewInput,
    out_dir: &std::path::Path,
) -> Result<(std::path::PathBuf, ReviewDecision), String> {
    let core = assess_review(pkg, input)?;
    let decision = ReviewDecision {
        schema_version: DECISION_SCHEMA_VERSION.to_string(),
        decision_id: decision_id_for(&core),
        decision_core: core.clone(),
        operational: super::operational::now_stamp(),
    };
    let dir = out_dir.join("review_decisions");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.json", core.finding_id));
    let json = serde_json::to_string_pretty(&decision).expect("decision serialization");
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok((path, decision))
}

/// Load a decision file (fail-closed on malformed content).
pub fn load_review_decision(path: &std::path::Path) -> Result<ReviewDecision, String> {
    let bytes = std::fs::read_to_string(path)
        .map_err(|e| format!("read decision {}: {e}", path.display()))?;
    let decision: ReviewDecision = serde_json::from_str(&bytes)
        .map_err(|e| format!("malformed decision {}: {e}", path.display()))?;
    if decision.schema_version != DECISION_SCHEMA_VERSION {
        return Err(format!(
            "unsupported decision schema '{}' (expected {})",
            decision.schema_version, DECISION_SCHEMA_VERSION
        ));
    }
    // The stored id must match the stored core (tamper detection).
    if decision.decision_id != decision_id_for(&decision.decision_core) {
        return Err(format!(
            "decision {} failed content-hash verification (tampered?)",
            path.display()
        ));
    }
    Ok(decision)
}

/// Re-verify a previously recorded decision against the CURRENT package
/// state. Used by the work order and the evidence chain: if the package
/// drifted since the human signed off, nothing downstream proceeds.
pub fn verify_decision_against_package(
    pkg: &EvidencePackage,
    core: &DecisionCore,
) -> Result<(), String> {
    if package_manifest_blake3(pkg) != core.package_manifest_blake3 {
        return Err(
            "package drifted since the decision was recorded (manifest hash mismatch)".to_string(),
        );
    }
    if pkg.manifest.workspace_snapshot_blake3 != core.workspace_snapshot_blake3 {
        return Err("workspace snapshot drifted since the decision was recorded".to_string());
    }
    let current = pkg.contract_hashes.get(&core.finding_id).ok_or_else(|| {
        format!(
            "contract for finding {} no longer present in the package",
            core.finding_id
        )
    })?;
    if current != &core.contract_blake3 {
        return Err(format!(
            "contract hash drifted since the decision was recorded (expected {}, package has {})",
            core.contract_blake3, current
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::pilot_package::load_evidence_package;
    use crate::analyzer::pilot_report::{build_bundle, write_bundle_artifacts};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const APPROVED_FINDING: &str = "MONEY-TRUNCATION-LEDGER-15";
    const CANDIDATE_ONLY_FINDING: &str = "TODO-MARKER-LEDGER-23";

    fn package_dir() -> tempfile::TempDir {
        let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_fintech");
        let mut repro = BTreeMap::new();
        repro.insert(APPROVED_FINDING.to_string(), "test_ledger.py".to_string());
        let bundle = build_bundle(&ws, &repro, &[]).unwrap();
        let out = tempfile::tempdir().unwrap();
        write_bundle_artifacts(&bundle, out.path()).unwrap();
        out
    }

    fn approve_input() -> ReviewInput {
        ReviewInput {
            finding_id: APPROVED_FINDING.to_string(),
            decision: DECISION_APPROVE.to_string(),
            reviewer: "pilot-operator".to_string(),
            rationale: "reviewed truncation evidence; repro test provided".to_string(),
            repro_test_path: Some("test_ledger.py".to_string()),
            override_candidate_only: false,
        }
    }

    #[test]
    fn approve_remediation_ready_finding_roundtrips() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let (path, decision) = record_review(&pkg, &approve_input(), dir.path()).unwrap();
        assert!(path.ends_with(format!("review_decisions/{APPROVED_FINDING}.json")));
        assert_eq!(decision.decision_core.decision, DECISION_APPROVE);
        assert!(!decision.decision_core.override_candidate_only);
        assert_eq!(decision.decision_id.len(), 64);
        // Stored file reloads and verifies.
        let back = load_review_decision(&path).unwrap();
        assert_eq!(back.decision_core, decision.decision_core);
        assert_eq!(back.decision_id, decision.decision_id);
        verify_decision_against_package(&pkg, &back.decision_core).unwrap();
    }

    #[test]
    fn decision_core_is_byte_stable_and_timestamp_free() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let a = assess_review(&pkg, &approve_input()).unwrap();
        let b = assess_review(&pkg, &approve_input()).unwrap();
        assert_eq!(decision_id_for(&a), decision_id_for(&b));
        let json = serde_json::to_string(&a).unwrap();
        for banned in ["timestamp", "recorded_at", "ts_unix"] {
            assert!(!json.contains(banned), "core must be timestamp-free");
        }
    }

    #[test]
    fn approve_candidate_only_requires_explicit_override() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let mut input = approve_input();
        input.finding_id = CANDIDATE_ONLY_FINDING.to_string();
        input.repro_test_path = None;
        let err = assess_review(&pkg, &input).unwrap_err();
        assert!(err.contains("override"), "{err}");

        input.override_candidate_only = true;
        input.rationale = "accepted residual risk after manual review".to_string();
        let core = assess_review(&pkg, &input).unwrap();
        assert!(core.override_candidate_only, "override must be recorded");
    }

    #[test]
    fn reject_and_defer_need_no_readiness() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        for verdict in [DECISION_REJECT, DECISION_DEFER] {
            let mut input = approve_input();
            input.finding_id = CANDIDATE_ONLY_FINDING.to_string();
            input.decision = verdict.to_string();
            input.repro_test_path = None;
            let core = assess_review(&pkg, &input).unwrap();
            assert_eq!(core.decision, verdict);
            assert!(!core.override_candidate_only);
        }
    }

    #[test]
    fn empty_reviewer_or_rationale_refused() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let mut input = approve_input();
        input.reviewer = "  ".to_string();
        assert!(assess_review(&pkg, &input).is_err());
        let mut input = approve_input();
        input.rationale = String::new();
        assert!(assess_review(&pkg, &input).is_err());
    }

    #[test]
    fn unknown_finding_and_invalid_decision_refused() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let mut input = approve_input();
        input.finding_id = "NO-SUCH-FINDING".to_string();
        assert!(assess_review(&pkg, &input).is_err());
        let mut input = approve_input();
        input.decision = "yolo".to_string();
        assert!(assess_review(&pkg, &input).is_err());
    }

    #[test]
    fn repro_outside_snapshot_refused() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let mut input = approve_input();
        input.repro_test_path = Some("no_such_test.py".to_string());
        let err = assess_review(&pkg, &input).unwrap_err();
        assert!(err.contains("not in the package snapshot"), "{err}");
    }

    #[test]
    fn package_drift_after_decision_is_detected() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let core = assess_review(&pkg, &approve_input()).unwrap();

        // Regenerate the package from a MODIFIED workspace copy: the
        // manifest hash changes → the recorded decision must no longer
        // verify against it.
        let tmp = tempfile::tempdir().unwrap();
        let ws2 = tmp.path().join("ws");
        std::fs::create_dir_all(ws2.join("billing")).unwrap();
        let src_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_fintech");
        for rel in [
            "billing/__init__.py",
            "billing/ledger.py",
            "billing/safe_pricing.py",
            "test_ledger.py",
            "README.md",
        ] {
            std::fs::copy(src_root.join(rel), ws2.join(rel)).unwrap();
        }
        // Drift the target file.
        let ledger = ws2.join("billing/ledger.py");
        std::fs::write(
            &ledger,
            std::fs::read_to_string(&ledger).unwrap() + "\n# drift\n",
        )
        .unwrap();
        let mut repro = BTreeMap::new();
        repro.insert(APPROVED_FINDING.to_string(), "test_ledger.py".to_string());
        let bundle2 = build_bundle(&ws2, &repro, &[]).unwrap();
        let out2 = tmp.path().join("pkg2");
        write_bundle_artifacts(&bundle2, &out2).unwrap();
        let pkg2 = load_evidence_package(&out2).unwrap();

        let err = verify_decision_against_package(&pkg2, &core).unwrap_err();
        assert!(err.contains("drifted"), "{err}");
    }

    #[test]
    fn tampered_decision_file_fails_reload() {
        let dir = package_dir();
        let pkg = load_evidence_package(dir.path()).unwrap();
        let (path, _) = record_review(&pkg, &approve_input(), dir.path()).unwrap();
        let mut content = std::fs::read_to_string(&path).unwrap();
        content = content.replace("pilot-operator", "someone-else");
        std::fs::write(&path, content).unwrap();
        let err = load_review_decision(&path).unwrap_err();
        assert!(err.contains("content-hash verification"), "{err}");
    }
}
