//! Remediation work order (v0.4-pilot-ops) — a PASSIVE handoff document.
//!
//! Generated only for APPROVED decisions. It tells the human operator
//! exactly what to carry to the executor side and what evidence to bring
//! back; it executes NOTHING and contains no code path that could invoke
//! the executor. Byte-stable: identical inputs ⇒ identical bytes, no
//! timestamps.

use serde::{Deserialize, Serialize};

use super::pilot_package::{blake3_hex, snapshot_hash_for, EvidencePackage};
use super::review_gate::{verify_decision_against_package, ReviewDecision, DECISION_APPROVE};
use super::ANALYZER_VERSION;

pub const WORK_ORDER_SCHEMA_VERSION: &str = "work_order_v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOrderTarget {
    pub path: String,
    /// Snapshot BLAKE3 the target file must match before any attempt.
    pub snapshot_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOrderCore {
    pub finding_id: String,
    pub decision_id: String,
    pub contract_blake3: String,
    /// Canonical TaskContract v0 JSON, embedded verbatim.
    pub contract_json: String,
    pub workspace_snapshot_blake3: String,
    pub target_files: Vec<WorkOrderTarget>,
    pub repro_test_path: Option<String>,
    pub analyzer_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOrder {
    pub schema_version: String,
    /// Content hash of `core`.
    pub work_order_id: String,
    pub core: WorkOrderCore,
    /// Static operator checklist (deterministic text).
    pub operator_instructions: Vec<String>,
}

const OPERATOR_INSTRUCTIONS: &[&str] = &[
    "1. Copy the target workspace to a fresh ISOLATED directory. Never run the executor on the original workspace.",
    "2. Hand the embedded TaskContract v0 JSON to the executor run in that isolated copy (manual pipeline-run; this document invokes nothing).",
    "3. The executor may honestly reject the contract (hallucinated context, failing validation). A rejection is a valid outcome.",
    "4. Preserve executor evidence: event log, BLAKE3 hashes of every target file BEFORE and AFTER the attempt, and test_report_v1.",
    "5. Bring the isolated pre-copy, post-copy and evidence files back and run analyzer_chain_verify.",
    "6. No production deployment of anything produced by the attempt.",
];

pub const WORK_ORDER_DISCLAIMER: &str =
    "This work order is a passive document. It executes nothing, invokes no executor, and does not modify any workspace.";

fn work_order_id_for(core: &WorkOrderCore) -> String {
    let json = serde_json::to_string(core).expect("work order core serialization");
    blake3_hex(&json)
}

/// Build the work order for an approved decision. Fail-closed when the
/// decision is not an approval or the package drifted since review.
pub fn build_work_order(
    pkg: &EvidencePackage,
    decision: &ReviewDecision,
) -> Result<WorkOrder, String> {
    let core = &decision.decision_core;
    if core.decision != DECISION_APPROVE {
        return Err(format!(
            "work order requires an approved decision; this decision is '{}'",
            core.decision
        ));
    }
    verify_decision_against_package(pkg, core)?;

    let task = pkg
        .tasks
        .iter()
        .find(|t| t.contract.finding.id == core.finding_id)
        .ok_or_else(|| format!("contract for {} missing from package", core.finding_id))?;

    let mut target_files = Vec::new();
    for rel in &task.contract.target_files {
        let hash = snapshot_hash_for(pkg, rel).ok_or_else(|| {
            format!("target file {rel} missing from the package snapshot inventory")
        })?;
        target_files.push(WorkOrderTarget {
            path: rel.clone(),
            snapshot_blake3: hash,
        });
    }
    target_files.sort_by(|a, b| a.path.cmp(&b.path));

    let contract_json = serde_json::to_string(&task.contract).expect("contract serialization");
    // Integrity: embedded bytes must hash to the decision's claim.
    if blake3_hex(&contract_json) != core.contract_blake3 {
        return Err("contract embedding failed integrity check".to_string());
    }

    let wo_core = WorkOrderCore {
        finding_id: core.finding_id.clone(),
        decision_id: decision.decision_id.clone(),
        contract_blake3: core.contract_blake3.clone(),
        contract_json,
        workspace_snapshot_blake3: core.workspace_snapshot_blake3.clone(),
        target_files,
        repro_test_path: core.repro_test_path.clone(),
        analyzer_version: ANALYZER_VERSION.to_string(),
    };
    Ok(WorkOrder {
        schema_version: WORK_ORDER_SCHEMA_VERSION.to_string(),
        work_order_id: work_order_id_for(&wo_core),
        core: wo_core,
        operator_instructions: OPERATOR_INSTRUCTIONS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    })
}

/// Deterministic markdown rendering of a work order.
pub fn render_work_order_md(order: &WorkOrder) -> String {
    let mut md = format!(
        "# Remediation Work Order ({finding})

- Work order id (content hash): `{woid}`
- Approved via decision id: `{did}`
- Contract BLAKE3: `{chash}`
- Workspace snapshot BLAKE3: `{snap}`
- Reproducible test: {repro}

{disclaimer}

## Target files (snapshot hashes must match before the attempt)

",
        finding = order.core.finding_id,
        woid = order.work_order_id,
        did = order.core.decision_id,
        chash = order.core.contract_blake3,
        snap = order.core.workspace_snapshot_blake3,
        repro = order
            .core
            .repro_test_path
            .clone()
            .unwrap_or_else(|| "(none)".to_string()),
        disclaimer = WORK_ORDER_DISCLAIMER
    );
    for t in &order.core.target_files {
        md.push_str(&format!(
            "- `{}` — BLAKE3 `{}`\n",
            t.path, t.snapshot_blake3
        ));
    }
    md.push_str("\n## Operator checklist\n\n");
    for step in &order.operator_instructions {
        md.push_str(&format!("{step}\n"));
    }
    md.push_str("\n## TaskContract v0 (verbatim, executor input)\n\n```json\n");
    md.push_str(&order.core.contract_json);
    md.push_str("\n```\n");
    md
}

/// Build and write `work_order_v1.json` + `WORK_ORDER.md` into `out_dir`.
pub fn write_work_order(
    pkg: &EvidencePackage,
    decision: &ReviewDecision,
    out_dir: &std::path::Path,
) -> Result<(std::path::PathBuf, std::path::PathBuf, WorkOrder), String> {
    let order = build_work_order(pkg, decision)?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let json_path = out_dir.join("work_order_v1.json");
    let md_path = out_dir.join("WORK_ORDER.md");
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&order).expect("work order serialization"),
    )
    .map_err(|e| format!("write {}: {e}", json_path.display()))?;
    std::fs::write(&md_path, render_work_order_md(&order))
        .map_err(|e| format!("write {}: {e}", md_path.display()))?;
    Ok((json_path, md_path, order))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::pilot_package::load_evidence_package;
    use crate::analyzer::pilot_report::{build_bundle, write_bundle_artifacts};
    use crate::analyzer::review_gate::{record_review, ReviewInput};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn approved_package() -> (tempfile::TempDir, EvidencePackage, ReviewDecision) {
        let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_fintech");
        let mut repro = BTreeMap::new();
        repro.insert(
            "MONEY-TRUNCATION-LEDGER-15".to_string(),
            "test_ledger.py".to_string(),
        );
        let bundle = build_bundle(&ws, &repro, &[]).unwrap();
        let out = tempfile::tempdir().unwrap();
        write_bundle_artifacts(&bundle, out.path()).unwrap();
        let pkg = load_evidence_package(out.path()).unwrap();
        let input = ReviewInput {
            finding_id: "MONEY-TRUNCATION-LEDGER-15".to_string(),
            decision: DECISION_APPROVE.to_string(),
            reviewer: "pilot-operator".to_string(),
            rationale: "approved for isolated attempt".to_string(),
            repro_test_path: Some("test_ledger.py".to_string()),
            override_candidate_only: false,
        };
        let (_, decision) = record_review(&pkg, &input, out.path()).unwrap();
        (out, pkg, decision)
    }

    #[test]
    fn work_order_builds_for_approved_decision() {
        let (_dir, pkg, decision) = approved_package();
        let order = build_work_order(&pkg, &decision).unwrap();
        assert_eq!(order.core.finding_id, "MONEY-TRUNCATION-LEDGER-15");
        assert_eq!(order.core.target_files.len(), 1);
        assert_eq!(order.core.target_files[0].path, "billing/ledger.py");
        assert_eq!(order.core.target_files[0].snapshot_blake3.len(), 64);
        assert!(
            order
                .core
                .contract_json
                .contains("\"task_kind\": \"codefix\"")
                || order
                    .core
                    .contract_json
                    .contains("\"task_kind\":\"codefix\"")
        );
        assert_eq!(order.work_order_id.len(), 64);
        let md = render_work_order_md(&order);
        assert!(md.contains(WORK_ORDER_DISCLAIMER));
        assert!(md.contains("## TaskContract v0"));
    }

    #[test]
    fn work_order_is_byte_stable() {
        let (_dir, pkg, decision) = approved_package();
        let a = build_work_order(&pkg, &decision).unwrap();
        let b = build_work_order(&pkg, &decision).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        assert_eq!(render_work_order_md(&a), render_work_order_md(&b));
    }

    #[test]
    fn non_approved_decision_refused() {
        let (_dir, pkg, mut decision) = approved_package();
        decision.decision_core.decision = "reject".to_string();
        // Note: decision_id no longer matches the core — load would catch
        // this on disk; build_work_order checks the decision field first.
        let err = build_work_order(&pkg, &decision).unwrap_err();
        assert!(err.contains("requires an approved decision"), "{err}");
    }

    #[test]
    fn drifted_package_refused() {
        let (_dir, pkg, decision) = approved_package();
        let mut drifted = pkg.clone();
        drifted.manifest_bytes.push(' ');
        let err = build_work_order(&drifted, &decision).unwrap_err();
        assert!(err.contains("drifted"), "{err}");
    }

    #[test]
    fn work_order_md_contains_no_forbidden_claims() {
        use crate::analyzer::pilot_report::FORBIDDEN_CLAIMS;
        let (_dir, pkg, decision) = approved_package();
        let order = build_work_order(&pkg, &decision).unwrap();
        let md = render_work_order_md(&order).to_lowercase();
        for banned in FORBIDDEN_CLAIMS {
            assert!(!md.contains(&banned.to_lowercase()), "forbidden: {banned}");
        }
    }
}
