//! Task emitter — converts findings into TaskContracts for the EXECUTOR.
//!
//! INVARIANT: a TaskContract only DESCRIBES work. It never performs
//! effects itself — patching, testing and validation happen exclusively
//! inside the executor (deterministic_ai_kernel_clean) through its
//! kernel-owned primitives, which do not trust this contract's claims.
//!
//! v0.2 adds a READINESS assessment next to each contract (the contract
//! itself stays byte-compatible with TaskContract v0). A static finding
//! without an operator-provided reproducible test is `candidate_only`;
//! the analyzer can never claim "fix ready" on its own.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::ingestion::WorkspaceInventory;
use super::monetary_oracle;
use super::triage::{Finding, Severity};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingBlock {
    pub id: String,
    pub severity: Severity,
    pub description: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProvenanceBlock {
    pub detector: String,
    pub confidence: f64,
    pub analyzer_version: String,
}

/// Contract v0 (ROLES.md): the exact JSON the executor consumes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskContract {
    pub task_kind: String,
    pub workspace: String,
    pub target_files: Vec<String>,
    pub finding: FindingBlock,
    pub codefix_steps: Vec<String>,
    pub tests_contract: String,
    pub provenance: ProvenanceBlock,
}

/// Readiness criteria attached to every emitted codefix task (v0.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadinessCriteria {
    /// A reproducible failing test exists AND was explicitly provided by
    /// the operator (e.g. `--repro <finding_id>=<path>`); the analyzer
    /// cannot verify test failure on its own (read-only invariant).
    pub reproducible_test_present: bool,
    /// Every evidence file still matches its snapshot BLAKE3 hash.
    pub target_file_snapshot_matches: bool,
    /// Evidence is complete: locations anchored, line ranges within the
    /// file, candidate statement and snippets non-empty.
    pub evidence_complete: bool,
    /// True iff the finding is NOT from a money-math rule, or a
    /// monetary-invariant test (marker-detected) guards it. Money-math
    /// findings WITHOUT an invariant test can never be remediation_ready.
    pub monetary_invariant_present: bool,
    /// Whether this finding came from a money-math rule (informational).
    pub is_monetary: bool,
    /// Always true: remediation requires human approval in this phase.
    pub manual_review_required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadinessAssessment {
    /// `candidate_only` | `remediation_ready`.
    pub readiness: String,
    pub criteria: ReadinessCriteria,
    /// The policy in human-readable form (audit trail).
    pub policy: String,
}

/// Emitted task = unchanged TaskContract v0 + readiness assessment.
/// The executor consumes only the `contract` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmittedTask {
    pub contract: TaskContract,
    pub readiness: ReadinessAssessment,
}

pub const READINESS_CANDIDATE_ONLY: &str = "candidate_only";
pub const READINESS_REMEDIATION_READY: &str = "remediation_ready";

const READINESS_POLICY: &str =
    "static finding without an operator-provided reproducible failing test is candidate_only; \
the analyzer never claims fix readiness on its own; manual review is always required; \
a money-math finding (money-truncation / money-round-bare / floor-div-money) is NEVER \
remediation_ready unless a monetary-invariant test (marker '# monetary-invariant: <finding_id>') \
guards it in the workspace — property-based checks are mandatory for money";

/// The standard executor CodeFix chain, anchored to the concrete target.
fn codefix_steps(workspace: &str, target: &str) -> Vec<String> {
    let path = format!("{workspace}/{target}");
    vec![
        format!("Step 1 read repository {path}"),
        "Step 2 find bug".to_string(),
        "Step 3 patch code".to_string(),
        "Step 4 apply patch".to_string(),
        "Step 5 run tests".to_string(),
        "Step 6 validate patch".to_string(),
    ]
}

fn target_files_for(f: &Finding) -> Vec<String> {
    f.evidence
        .iter()
        .filter_map(|e| e.split(':').next())
        .map(|s| s.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn build_contract(f: &Finding, workspace: &str) -> TaskContract {
    let target_files = target_files_for(f);
    let primary = target_files.first().cloned().unwrap_or_default();
    TaskContract {
        task_kind: "codefix".to_string(),
        workspace: workspace.to_string(),
        target_files,
        finding: FindingBlock {
            id: f.id.clone(),
            severity: f.severity,
            description: f.description.clone(),
            evidence: f.evidence.clone(),
        },
        codefix_steps: codefix_steps(workspace, &primary),
        tests_contract: "existing module tests + one new test reproducing this finding".to_string(),
        provenance: ProvenanceBlock {
            detector: f.provenance.detector.clone(),
            confidence: f.provenance.confidence,
            analyzer_version: f.provenance.analyzer_version.clone(),
        },
    }
}

/// Deterministic emission: identical findings ⇒ identical contracts.
pub fn emit_tasks(findings: &[Finding], workspace: &str) -> Vec<TaskContract> {
    findings
        .iter()
        .map(|f| build_contract(f, workspace))
        .collect()
}

/// Line count of a workspace file (read-only). `None` when unreadable —
/// evidence then counts as incomplete rather than crashing the emitter.
fn file_line_count(workspace: &str, rel: &str) -> Option<usize> {
    let path = std::path::Path::new(workspace).join(rel);
    let content = std::fs::read_to_string(path).ok()?;
    Some(content.lines().count())
}

/// Assess remediation readiness for one finding.
///
/// `repro_tests` maps finding id → operator-provided test file path
/// (workspace-relative). Only paths present in the snapshot count.
fn assess_readiness(
    f: &Finding,
    inv: &WorkspaceInventory,
    workspace: &str,
    repro_tests: &BTreeMap<String, String>,
) -> ReadinessAssessment {
    let hashes: BTreeMap<&str, &str> = inv
        .files
        .iter()
        .map(|e| (e.rel_path.as_str(), e.content_hash.as_str()))
        .collect();
    let known_paths: std::collections::BTreeSet<&str> =
        inv.files.iter().map(|e| e.rel_path.as_str()).collect();

    // Operator-provided reproducible test, verified to exist in snapshot.
    let reproducible_test_present = repro_tests
        .get(&f.id)
        .is_some_and(|p| known_paths.contains(p.as_str()));

    // Evidence files must match their snapshot hashes exactly.
    let target_file_snapshot_matches = f.evidence_locations.iter().all(|loc| {
        hashes
            .get(loc.file.as_str())
            .is_some_and(|h| *h == loc.file_blake3)
    });

    // Evidence completeness: anchored hashes, valid line ranges within
    // the actual file, non-empty candidate statement.
    let evidence_complete = target_file_snapshot_matches
        && !f.candidate_statement.is_empty()
        && f.evidence_locations.iter().all(|loc| {
            loc.line_start >= 1
                && loc.line_end >= loc.line_start
                && file_line_count(workspace, &loc.file).is_some_and(|n| loc.line_end <= n.max(1))
        });

    // Monetary hard gate: money-math findings require a property-based
    // invariant test in the workspace; non-monetary findings are not
    // gated by it (vacuously present).
    let is_monetary = monetary_oracle::is_monetary_rule(&f.rule_id);
    let monetary_invariant_present =
        !is_monetary || monetary_oracle::invariant_present(workspace, inv, &f.id);

    let ready = reproducible_test_present
        && target_file_snapshot_matches
        && evidence_complete
        && monetary_invariant_present;
    ReadinessAssessment {
        readiness: if ready {
            READINESS_REMEDIATION_READY.to_string()
        } else {
            READINESS_CANDIDATE_ONLY.to_string()
        },
        criteria: ReadinessCriteria {
            reproducible_test_present,
            target_file_snapshot_matches,
            evidence_complete,
            monetary_invariant_present,
            is_monetary,
            manual_review_required: true,
        },
        policy: READINESS_POLICY.to_string(),
    }
}

/// Deterministic emission with readiness: identical inputs ⇒ identical
/// output. The inner `contract` is byte-identical to `emit_tasks` output.
pub fn emit_tasks_with_readiness(
    findings: &[Finding],
    workspace: &str,
    inv: &WorkspaceInventory,
    repro_tests: &BTreeMap<String, String>,
) -> Vec<EmittedTask> {
    findings
        .iter()
        .map(|f| EmittedTask {
            contract: build_contract(f, workspace),
            readiness: assess_readiness(f, inv, workspace, repro_tests),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::ingestion::scan_workspace;
    use crate::analyzer::scan_primitives::run_static_scan;
    use crate::analyzer::triage::triage;
    use std::path::PathBuf;

    fn toy() -> (Vec<Finding>, WorkspaceInventory, String) {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/billing_python");
        let inv = scan_workspace(&root).unwrap();
        let findings = triage(&run_static_scan(&inv, &root), &inv);
        let ws = root.to_string_lossy().to_string();
        (findings, inv, ws)
    }

    #[test]
    fn contracts_match_contract_v0_shape() {
        let (findings, inv, ws) = toy();
        let tasks = emit_tasks(&findings, &ws);
        assert_eq!(tasks.len(), findings.len());
        for t in &tasks {
            assert_eq!(t.task_kind, "codefix");
            assert_eq!(t.codefix_steps.len(), 6);
            assert!(t.codefix_steps[0].starts_with("Step 1 read repository "));
            assert!(t.codefix_steps[0].contains(&ws));
            assert!(!t.target_files.is_empty());
            assert!(t.tests_contract.contains("existing module tests"));
            assert!(t.finding.evidence.iter().all(|e| e.contains(':')));
        }
        drop(inv);
    }

    #[test]
    fn emission_is_deterministic_and_json_stable() {
        let (findings, _inv, ws) = toy();
        let a = serde_json::to_string(&emit_tasks(&findings, &ws)).unwrap();
        let b = serde_json::to_string(&emit_tasks(&findings, &ws)).unwrap();
        assert_eq!(a, b, "contract JSON must be byte-stable");
    }

    #[test]
    fn without_operator_repro_everything_is_candidate_only() {
        let (findings, inv, ws) = toy();
        let empty = BTreeMap::new();
        let tasks = emit_tasks_with_readiness(&findings, &ws, &inv, &empty);
        assert!(!tasks.is_empty());
        for t in &tasks {
            assert_eq!(t.readiness.readiness, READINESS_CANDIDATE_ONLY);
            assert!(
                !t.readiness.criteria.reproducible_test_present,
                "analyzer must not self-claim a reproducible test"
            );
            assert!(t.readiness.criteria.manual_review_required);
            // Snapshots were taken from the same run → must match.
            assert!(t.readiness.criteria.target_file_snapshot_matches);
            assert!(t.readiness.criteria.evidence_complete);
        }
    }

    #[test]
    fn operator_provided_repro_test_promotes_to_remediation_ready() {
        // Uses a NON-monetary finding (none-arith) so the monetary-
        // invariant gate is vacuous; monetary promotion is covered by
        // money_finding_with_invariant_becomes_ready.
        let (findings, inv, ws) = toy();
        let finding = findings
            .iter()
            .find(|f| f.id.starts_with("NONE-ARITH"))
            .unwrap();
        let mut repro = BTreeMap::new();
        repro.insert(finding.id.clone(), "test_fees.py".to_string());
        let tasks = emit_tasks_with_readiness(&findings, &ws, &inv, &repro);
        let promoted = tasks
            .iter()
            .find(|t| t.contract.finding.id == finding.id)
            .unwrap();
        assert_eq!(promoted.readiness.readiness, READINESS_REMEDIATION_READY);
        assert!(promoted.readiness.criteria.reproducible_test_present);
        // Manual review stays mandatory even when remediation-ready.
        assert!(promoted.readiness.criteria.manual_review_required);
    }

    #[test]
    fn nonexistent_repro_path_does_not_promote() {
        let (findings, inv, ws) = toy();
        let mut repro = BTreeMap::new();
        repro.insert(findings[0].id.clone(), "no_such_test_file.py".to_string());
        let tasks = emit_tasks_with_readiness(&findings, &ws, &inv, &repro);
        assert_eq!(tasks[0].readiness.readiness, READINESS_CANDIDATE_ONLY);
        assert!(!tasks[0].readiness.criteria.reproducible_test_present);
    }

    #[test]
    fn inner_contract_is_byte_identical_to_v0_emission() {
        let (findings, inv, ws) = toy();
        let v0 = serde_json::to_string(&emit_tasks(&findings, &ws)).unwrap();
        let wrapped: Vec<TaskContract> =
            emit_tasks_with_readiness(&findings, &ws, &inv, &BTreeMap::new())
                .into_iter()
                .map(|t| t.contract)
                .collect();
        assert_eq!(v0, serde_json::to_string(&wrapped).unwrap());
    }

    // --- Monetary invariant hard gate ---

    /// Build a temp workspace with a truncation defect on a known line.
    /// `with_marker` controls whether the test file carries the
    /// `# monetary-invariant:` marker guarding the finding.
    fn money_ws(with_marker: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("billing")).unwrap();
        // Defect on line 3 -> finding id MONEY-TRUNCATION-FEES-3.
        std::fs::write(
            dir.path().join("billing/fees.py"),
            "def compute_fee(amount):\n    # fee in cents\n    return int(amount * 100) / 100\n",
        )
        .unwrap();
        let mut test = String::from(
            "from billing.fees import compute_fee\n\n\ndef test_fee():\n    assert compute_fee(0.125) == 0.13\n",
        );
        if with_marker {
            test.push_str("\n# monetary-invariant: MONEY-TRUNCATION-FEES-3\n");
        }
        std::fs::write(dir.path().join("test_fees.py"), test).unwrap();
        dir
    }

    fn emit_money(with_marker: bool) -> (Vec<super::EmittedTask>, tempfile::TempDir) {
        let dir = money_ws(with_marker);
        let inv = scan_workspace(dir.path()).unwrap();
        let findings = triage(&run_static_scan(&inv, dir.path()), &inv);
        let mut repro = BTreeMap::new();
        repro.insert(
            "MONEY-TRUNCATION-FEES-3".to_string(),
            "test_fees.py".to_string(),
        );
        let ws = dir.path().to_string_lossy().to_string();
        let tasks = emit_tasks_with_readiness(&findings, &ws, &inv, &repro);
        (tasks, dir)
    }

    #[test]
    fn money_finding_without_invariant_is_never_ready() {
        let (tasks, _dir) = emit_money(false);
        let t = tasks
            .iter()
            .find(|t| t.contract.finding.id == "MONEY-TRUNCATION-FEES-3")
            .expect("money finding present");
        assert!(t.readiness.criteria.is_monetary);
        assert!(!t.readiness.criteria.monetary_invariant_present);
        assert!(
            t.readiness.criteria.reproducible_test_present,
            "repro was provided"
        );
        assert_eq!(
            t.readiness.readiness, READINESS_CANDIDATE_ONLY,
            "money without invariant test must stay candidate_only even with a repro test"
        );
    }

    #[test]
    fn money_finding_with_invariant_becomes_ready() {
        let (tasks, _dir) = emit_money(true);
        let t = tasks
            .iter()
            .find(|t| t.contract.finding.id == "MONEY-TRUNCATION-FEES-3")
            .expect("money finding present");
        assert!(t.readiness.criteria.is_monetary);
        assert!(t.readiness.criteria.monetary_invariant_present);
        assert_eq!(t.readiness.readiness, READINESS_REMEDIATION_READY);
    }

    #[test]
    fn non_money_finding_not_gated_by_invariant() {
        // none-arith is not a money-math rule -> invariant gate is vacuous.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("billing")).unwrap();
        std::fs::write(
            dir.path().join("billing/net.py"),
            "def net(rec):\n    fee = rec.get(\"fee\")\n    return rec[\"gross\"] - fee\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("test_net.py"), "def test_n():\n    pass\n").unwrap();
        let inv = scan_workspace(dir.path()).unwrap();
        let findings = triage(&run_static_scan(&inv, dir.path()), &inv);
        let f = findings
            .iter()
            .find(|f| f.rule_id == "none-arith")
            .expect("none-arith present");
        let mut repro = BTreeMap::new();
        repro.insert(f.id.clone(), "test_net.py".to_string());
        let ws = dir.path().to_string_lossy().to_string();
        let tasks = emit_tasks_with_readiness(&findings, &ws, &inv, &repro);
        let t = tasks
            .iter()
            .find(|t| t.contract.finding.id == f.id)
            .unwrap();
        assert!(!t.readiness.criteria.is_monetary);
        assert!(
            t.readiness.criteria.monetary_invariant_present,
            "vacuously present for non-money"
        );
        assert_eq!(t.readiness.readiness, READINESS_REMEDIATION_READY);
    }
}
