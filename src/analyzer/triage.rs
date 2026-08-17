//! Triage — deterministic conversion of raw candidates into ranked
//! findings with severity, evidence and provenance.
//!
//! Determinism contract: identical candidates + identical snapshot ⇒
//! identical findings, in the same order. Severity rules are fixed
//! constants (no model, no clock).
//!
//! Findings are CANDIDATE STATEMENTS, not proven defects: every finding
//! carries explicit analysis limitations and provenance, and nothing
//! downstream may call a finding "fixed" without executor evidence.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::ingestion::WorkspaceInventory;
use super::scan_primitives::{rule_limitations, Candidate, DETECTOR_MODEL};
use super::ANALYZER_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    fn rank(self) -> u8 {
        match self {
            Severity::Critical => 3,
            Severity::High => 2,
            Severity::Medium => 1,
            Severity::Low => 0,
        }
    }
}

/// Allowed detector provenance values: `static` | `model` | `external`
/// (canonical constants live in `scan_primitives`, re-exported here for
/// API stability).
pub use super::scan_primitives::{DETECTOR_EXTERNAL, DETECTOR_STATIC};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// `static` | `model` | `external`.
    pub detector: String,
    pub confidence: f64,
    pub analyzer_version: String,
    /// Mandatory `true` for model-produced hints. Model output is never
    /// verified by the analyzer; deterministic static detectors are
    /// `false` here.
    pub model_hint_unverified: bool,
}

/// Provenance constructor enforcing the model-hint invariant: any
/// `model` detector is automatically marked unverified.
pub fn make_provenance(detector: &str, confidence: f64) -> Provenance {
    Provenance {
        detector: detector.to_string(),
        confidence,
        analyzer_version: ANALYZER_VERSION.to_string(),
        model_hint_unverified: detector == DETECTOR_MODEL,
    }
}

/// Concrete evidence location anchored to the workspace snapshot: the
/// BLAKE3 hash is taken from the inventory, so a later drift of the file
/// is detectable before any remediation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceLocation {
    pub file: String,
    pub line_start: usize,
    pub line_end: usize,
    pub file_blake3: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Stable finding id (rule + file stem + line).
    pub id: String,
    pub rule_id: String,
    pub severity: Severity,
    /// Explicitly a CANDIDATE statement, never a proven defect claim.
    pub candidate_statement: String,
    /// Kept for TaskContract v0 compatibility (equals
    /// `candidate_statement`).
    pub description: String,
    /// `file:line_start-line_end` evidence coordinates (v0 format).
    pub evidence: Vec<String>,
    /// Snapshot-anchored evidence locations (v0.2).
    pub evidence_locations: Vec<EvidenceLocation>,
    /// Known false-positive/false-negative profile of the detector.
    pub limitations: Vec<String>,
    pub provenance: Provenance,
}

/// Money-domain paths deserve stricter severity.
fn is_money_path(file: &str) -> bool {
    let lower = file.to_lowercase();
    ["billing", "fee", "payment", "charge", "price", "invoice"]
        .iter()
        .any(|k| lower.contains(k))
}

/// Conservative mapping for external (tool-reported) severities: an
/// external tool alone never justifies Critical, and its confidence is
/// capped below the deterministic static rules.
fn classify_external(external_severity: Option<&str>) -> (Severity, f64) {
    match external_severity.map(|s| s.to_uppercase()) {
        Some(s) if s == "ERROR" || s == "HIGH" => (Severity::High, 0.55),
        Some(s) if s == "WARNING" || s == "MEDIUM" => (Severity::Medium, 0.45),
        _ => (Severity::Low, 0.35),
    }
}

/// Fixed severity/confidence table per rule (deterministic).
fn classify(candidate: &Candidate) -> (Severity, f64) {
    if candidate.detector == DETECTOR_EXTERNAL {
        return classify_external(candidate.external_severity.as_deref());
    }
    let money = is_money_path(&candidate.file);
    match candidate.rule_id.as_str() {
        "dangerous-eval" => (Severity::Critical, 0.9),
        "money-truncation" | "money-round-bare" => {
            if money {
                (Severity::Critical, 0.7)
            } else {
                (Severity::High, 0.6)
            }
        }
        "offbyone-range" => {
            if money {
                (Severity::High, 0.65)
            } else {
                (Severity::Medium, 0.5)
            }
        }
        "none-arith" => {
            if money {
                (Severity::High, 0.6)
            } else {
                (Severity::Medium, 0.5)
            }
        }
        "todo-marker" => (Severity::Low, 0.3),
        "float-equality" => {
            if money {
                (Severity::High, 0.55)
            } else {
                (Severity::Medium, 0.45)
            }
        }
        "floor-div-money" => {
            if money {
                (Severity::High, 0.6)
            } else {
                (Severity::Medium, 0.5)
            }
        }
        _ => (Severity::Low, 0.2),
    }
}

/// Deterministic tie-breaker priority: deterministic static evidence
/// outranks external tool hints, which outrank model hints.
fn detector_rank(detector: &str) -> u8 {
    match detector {
        DETECTOR_STATIC => 0,
        DETECTOR_EXTERNAL => 1,
        DETECTOR_MODEL => 2,
        _ => 3,
    }
}

fn finding_id(candidate: &Candidate) -> String {
    let stem = candidate
        .file
        .rsplit('/')
        .next()
        .unwrap_or(&candidate.file)
        .split('.')
        .next()
        .unwrap_or("file");
    format!(
        "{}-{}-{}",
        candidate.rule_id.to_uppercase(),
        stem.to_uppercase(),
        candidate.line_start
    )
}

/// Deterministic triage: candidates → severity-ranked findings anchored
/// to the workspace snapshot (`inv` supplies per-file BLAKE3 hashes).
pub fn triage(candidates: &[Candidate], inv: &WorkspaceInventory) -> Vec<Finding> {
    let hashes: BTreeMap<&str, &str> = inv
        .files
        .iter()
        .map(|f| (f.rel_path.as_str(), f.content_hash.as_str()))
        .collect();
    let mut findings: Vec<Finding> = candidates
        .iter()
        .map(|c| {
            let (severity, confidence) = classify(c);
            let file_blake3 = hashes
                .get(c.file.as_str())
                .copied()
                .unwrap_or("missing-from-snapshot")
                .to_string();
            Finding {
                id: finding_id(c),
                rule_id: c.rule_id.clone(),
                severity,
                candidate_statement: c.suspicion.clone(),
                description: c.suspicion.clone(),
                evidence: vec![format!("{}:{}-{}", c.file, c.line_start, c.line_end)],
                evidence_locations: vec![EvidenceLocation {
                    file: c.file.clone(),
                    line_start: c.line_start,
                    line_end: c.line_end,
                    file_blake3,
                }],
                limitations: rule_limitations(&c.rule_id),
                provenance: make_provenance(&c.detector, confidence),
            }
        })
        .collect();
    // Rank: severity desc, then detector priority (static < external <
    // model), then evidence, then id — fully deterministic.
    findings.sort_by(|a, b| {
        b.severity
            .rank()
            .cmp(&a.severity.rank())
            .then_with(|| {
                detector_rank(&a.provenance.detector).cmp(&detector_rank(&b.provenance.detector))
            })
            .then_with(|| a.evidence.cmp(&b.evidence))
            .then_with(|| a.id.cmp(&b.id))
    });
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::ingestion::scan_workspace;
    use crate::analyzer::scan_primitives::run_static_scan;
    use std::path::PathBuf;

    fn toy_findings() -> Vec<Finding> {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/billing_python");
        let inv = scan_workspace(&root).unwrap();
        triage(&run_static_scan(&inv, &root), &inv)
    }

    #[test]
    fn triage_ranks_money_defects_highest() {
        let findings = toy_findings();
        assert!(findings.len() >= 3, "expected >=3 findings");
        // The two money defects must outrank everything else.
        let top_two: Vec<&Severity> = findings.iter().take(2).map(|f| &f.severity).collect();
        assert!(
            top_two
                .iter()
                .all(|s| **s == Severity::Critical || **s == Severity::High),
            "money defects must rank on top: {findings:?}"
        );
        assert!(findings
            .iter()
            .any(|f| f.id.starts_with("MONEY-TRUNCATION")));
        assert!(findings.iter().any(|f| f.id.starts_with("OFFBYONE-RANGE")));
        assert!(findings.iter().any(|f| f.id.starts_with("NONE-ARITH")));
    }

    #[test]
    fn triage_is_deterministic() {
        assert_eq!(toy_findings(), toy_findings());
    }

    #[test]
    fn findings_carry_provenance_and_evidence() {
        for f in toy_findings() {
            assert_eq!(f.provenance.detector, DETECTOR_STATIC);
            assert!(f.provenance.confidence > 0.0);
            assert_eq!(f.provenance.analyzer_version, ANALYZER_VERSION);
            assert!(
                !f.provenance.model_hint_unverified,
                "static findings are not model hints"
            );
            assert!(f.evidence.iter().all(|e| e.contains(':')));
        }
    }

    #[test]
    fn findings_carry_snapshot_anchored_locations_and_limitations() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/billing_python");
        let inv = scan_workspace(&root).unwrap();
        let findings = triage(&run_static_scan(&inv, &root), &inv);
        for f in &findings {
            assert_eq!(f.evidence_locations.len(), 1);
            let loc = &f.evidence_locations[0];
            assert_eq!(loc.file_blake3.len(), 64, "real snapshot hash expected");
            assert_ne!(loc.file_blake3, "missing-from-snapshot");
            assert!(!f.limitations.is_empty(), "every finding discloses FP/FN");
            assert_eq!(f.candidate_statement, f.description);
            assert!(!f.rule_id.is_empty());
        }
    }

    #[test]
    fn model_hints_are_always_marked_unverified() {
        let p = make_provenance(DETECTOR_MODEL, 0.4);
        assert!(p.model_hint_unverified);
        let s = make_provenance(DETECTOR_STATIC, 0.7);
        assert!(!s.model_hint_unverified);
        let e = make_provenance(DETECTOR_EXTERNAL, 0.5);
        assert!(!e.model_hint_unverified);
    }

    #[test]
    fn external_severity_mapping_is_conservative_and_deterministic() {
        assert_eq!(classify_external(Some("ERROR")), (Severity::High, 0.55));
        assert_eq!(classify_external(Some("HIGH")), (Severity::High, 0.55));
        assert_eq!(classify_external(Some("WARNING")), (Severity::Medium, 0.45));
        assert_eq!(classify_external(Some("MEDIUM")), (Severity::Medium, 0.45));
        assert_eq!(classify_external(Some("LOW")), (Severity::Low, 0.35));
        assert_eq!(classify_external(Some("INFO")), (Severity::Low, 0.35));
        assert_eq!(classify_external(None), (Severity::Low, 0.35));
        // External never reaches Critical on its own.
        assert!(classify_external(Some("ERROR")).0 < Severity::Critical);
    }

    #[test]
    fn static_findings_outrank_external_at_equal_severity() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/billing_python");
        let inv = scan_workspace(&root).unwrap();
        let mut cands = run_static_scan(&inv, &root);
        cands.push(Candidate {
            rule_id: "semgrep:test-check".to_string(),
            file: "billing/fees.py".to_string(),
            line_start: 5,
            line_end: 5,
            suspicion: "external hint".to_string(),
            snippet: "x = 1".to_string(),
            detector: DETECTOR_EXTERNAL.to_string(),
            external_severity: Some("HIGH".to_string()),
        });
        let findings = triage(&cands, &inv);
        let highs: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.severity == Severity::High)
            .collect();
        assert!(highs.len() >= 2);
        assert_eq!(
            highs[0].provenance.detector, DETECTOR_STATIC,
            "static must outrank external at equal severity"
        );
        let ext = highs
            .iter()
            .find(|f| f.provenance.detector == DETECTOR_EXTERNAL)
            .expect("external finding present");
        assert!(
            ext.provenance.confidence < 0.6,
            "external confidence capped"
        );
        assert!(!ext.provenance.model_hint_unverified);
        assert!(ext
            .limitations
            .iter()
            .any(|l| l.contains("external SAST finding")));
    }
}
