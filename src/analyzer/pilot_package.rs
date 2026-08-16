//! Evidence package loading — read access to a pilot output directory
//! produced by `analyzer_pilot_report`.
//!
//! Everything downstream of v0.3 (review gate, work order, evidence
//! chain) starts here. Contract hashes are RECOMPUTED from the contract
//! bytes on load — never copied from any report text — so a tampered or
//! stale package is detected before any decision is recorded.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::evidence_manifest::EvidenceManifest;
use super::task_emitter::EmittedTask;
use super::triage::Finding;

pub const MANIFEST_FILE: &str = "evidence_manifest_v1.json";
pub const FINDINGS_FILE: &str = "findings_v1.json";
pub const CONTRACTS_FILE: &str = "task_contracts_v0.json";

/// A loaded, integrity-checked evidence package.
#[derive(Debug, Clone)]
pub struct EvidencePackage {
    pub dir: PathBuf,
    /// Raw manifest bytes (the BLAKE3 anchor for decisions).
    pub manifest_bytes: String,
    pub manifest: EvidenceManifest,
    pub findings: Vec<Finding>,
    pub tasks: Vec<EmittedTask>,
    /// finding id → BLAKE3 of the canonical TaskContract v0 JSON,
    /// recomputed from the package bytes.
    pub contract_hashes: BTreeMap<String, String>,
}

/// BLAKE3 hex of a string payload.
pub fn blake3_hex(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

fn read_required(dir: &Path, name: &str) -> Result<String, String> {
    let path = dir.join(name);
    std::fs::read_to_string(&path)
        .map_err(|e| format!("evidence package is incomplete ({}): {e}", path.display()))
}

/// Load and integrity-check an evidence package directory. Fails closed
/// on missing files, malformed JSON, or a finding/contract mismatch.
pub fn load_evidence_package(dir: &Path) -> Result<EvidencePackage, String> {
    let manifest_bytes = read_required(dir, MANIFEST_FILE)?;
    let findings_bytes = read_required(dir, FINDINGS_FILE)?;
    let tasks_bytes = read_required(dir, CONTRACTS_FILE)?;

    let manifest: EvidenceManifest = serde_json::from_str(&manifest_bytes)
        .map_err(|e| format!("{} is malformed: {e}", MANIFEST_FILE))?;
    let findings: Vec<Finding> = serde_json::from_str(&findings_bytes)
        .map_err(|e| format!("{} is malformed: {e}", FINDINGS_FILE))?;
    let tasks: Vec<EmittedTask> = serde_json::from_str(&tasks_bytes)
        .map_err(|e| format!("{} is malformed: {e}", CONTRACTS_FILE))?;

    // Recompute contract hashes from the canonical contract bytes.
    let mut contract_hashes = BTreeMap::new();
    for task in &tasks {
        let json = serde_json::to_string(&task.contract)
            .map_err(|e| format!("contract serialization: {e}"))?;
        contract_hashes.insert(task.contract.finding.id.clone(), blake3_hex(&json));
    }
    // Cross-check: every emitted contract must correspond to a finding.
    for task in &tasks {
        let id = &task.contract.finding.id;
        if !findings.iter().any(|f| &f.id == id) {
            return Err(format!(
                "package integrity: contract {id} has no matching finding"
            ));
        }
    }

    Ok(EvidencePackage {
        dir: dir.to_path_buf(),
        manifest_bytes,
        manifest,
        findings,
        tasks,
        contract_hashes,
    })
}

/// BLAKE3 of the manifest bytes as stored in the package.
pub fn package_manifest_blake3(pkg: &EvidencePackage) -> String {
    blake3_hex(&pkg.manifest_bytes)
}

/// Inventory hash of a workspace-relative path (None when absent).
pub fn snapshot_hash_for(pkg: &EvidencePackage, rel_path: &str) -> Option<String> {
    pkg.manifest
        .inventory
        .included_files
        .iter()
        .find(|f| f.path == rel_path)
        .map(|f| f.blake3.clone())
}

/// Internal package consistency for one finding: every evidence location
/// must carry the exact snapshot hash recorded in the manifest. This
/// catches packages whose findings were modified after emission.
pub fn verify_finding_anchoring(pkg: &EvidencePackage, finding_id: &str) -> Result<(), String> {
    let finding = pkg
        .findings
        .iter()
        .find(|f| f.id == finding_id)
        .ok_or_else(|| format!("finding {finding_id} not present in this package"))?;
    for loc in &finding.evidence_locations {
        let expected = snapshot_hash_for(pkg, &loc.file).ok_or_else(|| {
            format!(
                "package integrity: evidence file {} of {} missing from manifest inventory",
                loc.file, finding_id
            )
        })?;
        if expected != loc.file_blake3 {
            return Err(format!(
                "package integrity: evidence hash of {} ({}) does not match manifest snapshot ({})",
                finding_id, loc.file_blake3, expected
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::pilot_report::{build_bundle, write_bundle_artifacts};

    fn write_pilot_package() -> tempfile::TempDir {
        let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_fintech");
        let mut repro = BTreeMap::new();
        repro.insert(
            "MONEY-TRUNCATION-LEDGER-15".to_string(),
            "test_ledger.py".to_string(),
        );
        let bundle = build_bundle(&ws, &repro, &[]).unwrap();
        let out = tempfile::tempdir().unwrap();
        write_bundle_artifacts(&bundle, out.path()).unwrap();
        out
    }

    #[test]
    fn package_loads_with_recomputed_contract_hashes() {
        let out = write_pilot_package();
        let pkg = load_evidence_package(out.path()).unwrap();
        assert_eq!(pkg.findings.len(), 3);
        assert_eq!(pkg.tasks.len(), 3);
        assert_eq!(pkg.contract_hashes.len(), 3);
        for (id, hash) in &pkg.contract_hashes {
            assert_eq!(hash.len(), 64, "{id} hash must be BLAKE3 hex");
        }
        assert_eq!(package_manifest_blake3(&pkg).len(), 64);
    }

    #[test]
    fn finding_anchoring_detects_tampered_evidence_hash() {
        let out = write_pilot_package();
        // Tamper: corrupt one finding's snapshot hash on disk.
        let findings_path = out.path().join(FINDINGS_FILE);
        let mut content = std::fs::read_to_string(&findings_path).unwrap();
        let original = content.clone();
        let first_hash_start = content.find("\"file_blake3\": \"").unwrap() + 16;
        let hash_end = first_hash_start + 64;
        let mut tampered_hash = content[first_hash_start..hash_end].to_string();
        tampered_hash.replace_range(
            0..1,
            if tampered_hash.starts_with('0') {
                "1"
            } else {
                "0"
            },
        );
        content.replace_range(first_hash_start..hash_end, &tampered_hash);
        assert_ne!(content, original);
        std::fs::write(&findings_path, content).unwrap();

        let pkg = load_evidence_package(out.path()).unwrap();
        let finding = &pkg.findings[0];
        let err = verify_finding_anchoring(&pkg, &finding.id).unwrap_err();
        assert!(err.contains("package integrity"), "{err}");
    }

    #[test]
    fn missing_artifact_fails_closed() {
        let out = write_pilot_package();
        std::fs::remove_file(out.path().join(CONTRACTS_FILE)).unwrap();
        assert!(load_evidence_package(out.path()).is_err());
    }
}
