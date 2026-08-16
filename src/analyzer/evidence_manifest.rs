//! Evidence manifest — byte-stable, tamper-evident description of one
//! analyzer run (schema `evidence_manifest_v1`).
//!
//! Determinism contract:
//! - NO timestamps inside the manifest (operational time belongs in the
//!   separate operational audit log, which is excluded from content
//!   hashes).
//! - Files and rule ids are sorted; the `workspace_snapshot_blake3` is a
//!   BLAKE3 fold over the sorted `(rel_path, file_blake3)` pairs, so any
//!   content drift changes the snapshot hash.
//! - `run_id` is a content hash of the manifest itself (with the run id
//!   field empty), so two identical inputs always produce the same id.

use serde::{Deserialize, Serialize};

use super::ingestion::WorkspaceInventory;
use super::scan_primitives::{ruleset_ids, RULESET_VERSION};
use super::ANALYZER_VERSION;

pub const MANIFEST_SCHEMA_VERSION: &str = "evidence_manifest_v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestFileEntry {
    pub path: String,
    pub language: String,
    pub bytes: u64,
    pub blake3: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestInventory {
    pub file_count: usize,
    pub included_files: Vec<ManifestFileEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestRuleset {
    pub version: String,
    pub rule_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestRun {
    /// Deterministic id: content hash of this manifest (run id excluded).
    pub run_id: String,
}

/// One external SAST report consumed (read-only) during the run
/// (v0.3). The report BLAKE3 anchors which external input produced the
/// external candidates — part of the tamper-evident audit trail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalSource {
    /// `semgrep` | `bandit`.
    pub tool: String,
    pub report_blake3: String,
    pub candidates: usize,
    pub rejected_paths: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceManifest {
    pub schema_version: String,
    pub analyzer_version: String,
    pub workspace: String,
    pub workspace_snapshot_blake3: String,
    pub inventory: ManifestInventory,
    pub ruleset: ManifestRuleset,
    /// External candidate sources; empty for static-only runs.
    pub external_sources: Vec<ExternalSource>,
    pub run: ManifestRun,
}

/// BLAKE3 over the canonical snapshot description: sorted
/// `rel_path \x00 content_blake3 \n` records. Any added, removed,
/// renamed or modified file changes this hash.
pub fn workspace_snapshot_hash(inv: &WorkspaceInventory) -> String {
    let mut hasher = blake3::Hasher::new();
    for entry in &inv.files {
        hasher.update(entry.rel_path.as_bytes());
        hasher.update(b"\x00");
        hasher.update(entry.content_hash.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().to_hex().to_string()
}

/// Canonical JSON used for the run id: serialization order follows
/// struct declaration order and all collections are sorted upstream, so
/// this is byte-stable for identical inputs.
fn canonical_json(m: &EvidenceManifest) -> String {
    serde_json::to_string(m).expect("manifest serialization cannot fail")
}

/// Build the byte-stable manifest for one analyzer run. `workspace` is
/// the canonicalized workspace path string (kept out of the hash base:
/// the snapshot hash covers content, the path is operator context).
/// `external_sources` is sorted deterministically (tool, then hash).
pub fn build_manifest(
    inv: &WorkspaceInventory,
    workspace: &str,
    mut external_sources: Vec<ExternalSource>,
) -> EvidenceManifest {
    external_sources.sort_by(|a, b| {
        a.tool
            .cmp(&b.tool)
            .then_with(|| a.report_blake3.cmp(&b.report_blake3))
    });
    let mut included: Vec<ManifestFileEntry> = inv
        .files
        .iter()
        .map(|f| ManifestFileEntry {
            path: f.rel_path.clone(),
            language: f.language.clone(),
            bytes: f.bytes,
            blake3: f.content_hash.clone(),
        })
        .collect();
    included.sort_by(|a, b| a.path.cmp(&b.path));

    let mut manifest = EvidenceManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        analyzer_version: ANALYZER_VERSION.to_string(),
        workspace: workspace.to_string(),
        workspace_snapshot_blake3: workspace_snapshot_hash(inv),
        inventory: ManifestInventory {
            file_count: included.len(),
            included_files: included,
        },
        ruleset: ManifestRuleset {
            version: RULESET_VERSION.to_string(),
            rule_ids: ruleset_ids(),
        },
        external_sources,
        run: ManifestRun {
            run_id: String::new(),
        },
    };
    let content_hash = blake3::hash(canonical_json(&manifest).as_bytes())
        .to_hex()
        .to_string();
    manifest.run.run_id = content_hash;
    manifest
}

/// Byte-stable pretty JSON of the manifest (no timestamps anywhere).
pub fn manifest_json(manifest: &EvidenceManifest) -> String {
    serde_json::to_string_pretty(manifest).expect("manifest serialization cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::ingestion::scan_workspace;
    use std::path::PathBuf;

    fn toy_inventory() -> (WorkspaceInventory, String) {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/billing_python");
        let ws = root.to_string_lossy().to_string();
        (scan_workspace(&root).unwrap(), ws)
    }

    #[test]
    fn manifest_is_byte_identical_across_repeats() {
        let (inv, ws) = toy_inventory();
        let a = manifest_json(&build_manifest(&inv, &ws, Vec::new()));
        let b = manifest_json(&build_manifest(&inv, &ws, Vec::new()));
        assert_eq!(a, b, "manifest must be byte-stable");
    }

    #[test]
    fn manifest_has_schema_and_no_timestamps() {
        let (inv, ws) = toy_inventory();
        let json = manifest_json(&build_manifest(&inv, &ws, Vec::new()));
        assert!(json.contains("\"schema_version\": \"evidence_manifest_v1\""));
        assert!(json.contains("\"workspace_snapshot_blake3\""));
        for banned in ["timestamp", "started_at", "ts_unix", "duration"] {
            assert!(
                !json.contains(banned),
                "manifest must not carry time metadata: {banned}"
            );
        }
    }

    #[test]
    fn run_id_is_content_hash_and_stable() {
        let (inv, ws) = toy_inventory();
        let m1 = build_manifest(&inv, &ws, Vec::new());
        let m2 = build_manifest(&inv, &ws, Vec::new());
        assert_eq!(m1.run.run_id, m2.run.run_id);
        assert_eq!(m1.run.run_id.len(), 64, "BLAKE3 hex expected");
    }

    #[test]
    fn external_sources_become_part_of_the_manifest_and_run_id() {
        let (inv, ws) = toy_inventory();
        let src = ExternalSource {
            tool: "semgrep".to_string(),
            report_blake3: "ab".repeat(32),
            candidates: 1,
            rejected_paths: 1,
        };
        let plain = build_manifest(&inv, &ws, Vec::new());
        let with_ext1 = build_manifest(&inv, &ws, vec![src.clone()]);
        let with_ext2 = build_manifest(&inv, &ws, vec![src.clone()]);
        assert_eq!(with_ext1, with_ext2, "byte-stable with externals");
        assert_ne!(
            plain.run.run_id, with_ext1.run.run_id,
            "external input must change the run identity"
        );
        assert_eq!(with_ext1.external_sources, vec![src.clone()]);
        assert!(manifest_json(&with_ext1).contains("\"external_sources\""));
        // Sorting: (tool, report_blake3) regardless of input order.
        let other = ExternalSource {
            tool: "bandit".to_string(),
            report_blake3: "cd".repeat(32),
            candidates: 2,
            rejected_paths: 0,
        };
        let ab = build_manifest(&inv, &ws, vec![src.clone(), other.clone()]);
        let ba = build_manifest(&inv, &ws, vec![other, src]);
        assert_eq!(ab, ba);
    }

    #[test]
    fn snapshot_hash_changes_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("billing/fee.py");
        std::fs::create_dir_all(dir.path().join("billing")).unwrap();
        std::fs::write(&file, "x = 1\n").unwrap();
        let inv1 = scan_workspace(dir.path()).unwrap();
        let h1 = workspace_snapshot_hash(&inv1);
        std::fs::write(&file, "x = 2\n").unwrap();
        let inv2 = scan_workspace(dir.path()).unwrap();
        let h2 = workspace_snapshot_hash(&inv2);
        assert_ne!(h1, h2, "content drift must change snapshot hash");
    }

    #[test]
    fn generated_and_vcs_paths_are_excluded_from_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("billing")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::create_dir_all(root.join("venv")).unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join("__pycache__")).unwrap();
        std::fs::create_dir_all(root.join("analyzer_out")).unwrap();
        std::fs::write(root.join("billing/fee.py"), "x = 1\n").unwrap();
        std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
        std::fs::write(root.join("target/out.bin"), "zz").unwrap();
        std::fs::write(root.join("venv/pyvenv.cfg"), "home = x\n").unwrap();
        std::fs::write(root.join("node_modules/x.js"), "//").unwrap();
        std::fs::write(root.join("__pycache__/f.pyc"), "zz").unwrap();
        std::fs::write(root.join("analyzer_out/findings.json"), "{}").unwrap();

        let inv = scan_workspace(root).unwrap();
        let manifest = build_manifest(&inv, &root.to_string_lossy(), Vec::new());
        let paths: Vec<&str> = manifest
            .inventory
            .included_files
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["billing/fee.py"],
            "only source files: {paths:?}"
        );
        assert_eq!(manifest.inventory.file_count, 1);
    }
}
