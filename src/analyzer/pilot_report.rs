//! Pilot report bundle — pure construction of the Enterprise Pilot
//! Evidence Package artifacts (v0.2).
//!
//! Everything here is CLOCK-FREE: the bundle (manifest, findings,
//! contracts, report) is byte-stable for identical workspace + analyzer
//! version + readiness inputs. Timestamps and durations are added only
//! by the caller in `OperationalRunLog`, which never participates in
//! content hashes.
//!
//! READ-ONLY invariant: this module reads the workspace and produces
//! in-memory artifacts; writing to disk is done by the caller into the
//! analyzer's own output directory, never into the workspace.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use super::audit_log::RunRecord;
use super::task_emitter::{EmittedTask, READINESS_REMEDIATION_READY};
use super::triage::Finding;
use super::{evidence_manifest, ingestion, scan_primitives, task_emitter, triage};

/// Claims that must never appear in a pilot report (tested).
pub const FORBIDDEN_CLAIMS: &[&str] = &[
    "guarantees security",
    "guarantees compliance",
    "guarantee the absence of defects",
    "automatically fixes all",
    "automatically fixes any",
    "certifies the absence of errors",
    "hallucinations are fully eliminated",
    "hallucinations are eliminated completely",
];

/// One fully-built pilot evidence bundle (all deterministic parts).
#[derive(Debug, Clone)]
pub struct PilotBundle {
    pub manifest: evidence_manifest::EvidenceManifest,
    pub findings: Vec<Finding>,
    pub tasks: Vec<EmittedTask>,
    pub manifest_json: String,
    pub findings_json: String,
    pub tasks_json: String,
    pub report_md: String,
    /// finding id → BLAKE3 of the canonical TaskContract v0 JSON.
    pub contract_hashes: BTreeMap<String, String>,
}

/// Operational (NON byte-stable) run log: the only place where time
/// metadata lives. Kept strictly separate from content-hashed artifacts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalRunLog {
    pub started_at_iso: String,
    pub ts_unix: u64,
    pub duration_ms: u64,
    pub record: RunRecord,
    /// artifact file name → BLAKE3 of the written bytes.
    pub artifacts: BTreeMap<String, String>,
}

/// Why each rule matters, in client-facing terms.
fn why_it_matters(rule_id: &str) -> &'static str {
    match rule_id {
        "money-truncation" => "sub-cent fractions are silently dropped; repeated over many transactions this becomes systematic money loss",
        "money-round-bare" => "rounding without a declared policy produces banker's-rounding surprises on amounts customers are charged or credited",
        "offbyone-range" => "a skipped boundary element can mean a tier, limit or bracket is never applied",
        "none-arith" => "a missing value flowing into arithmetic crashes settlement/fee paths at runtime",
        "todo-marker" => "unfinished logic on a money/risk path is unquantified operational risk",
        "dangerous-eval" => "eval/exec can execute attacker-controlled code",
        _ => "see rule catalog",
    }
}

fn render_scope_section(manifest: &evidence_manifest::EvidenceManifest) -> String {
    let rule_list = manifest.ruleset.rule_ids.join(", ");
    format!(
        "## 1. Scope and limitations

- Workspace: `{workspace}`
- Workspace snapshot BLAKE3: `{snap}`
- Files inventoried: {count}
- Ruleset: `{ruleset}` — rules: {rule_list}
- Run id (content hash): `{run_id}`

**This is read-only candidate detection, not a guarantee of absence of
defects.** A finding is a candidate statement, not a proven defect.

Model hints (if enabled in future runs) are unverified: the model never
executes commands and never modifies code. This pilot run uses the
deterministic static pipeline only.

Supported scope: **Python-first demo/risk logic only.** Explicit
limitations:

- No multi-language coverage (JVM/Go/JS are backlog, not supported).
- No repo-scale semantic retrieval — pattern-level static rules only.
- Every finding must pass human review before any remediation attempt.
- The LLM used by the executor can still be wrong outside the kernel's
  gated classes; the executor's gates contain, not eliminate, that risk.
- Rules carry documented false-positive/false-negative profiles (see
  section 2 and docs/ANALYZER_RULES_CATALOG.md).
",
        workspace = manifest.workspace,
        snap = manifest.workspace_snapshot_blake3,
        count = manifest.inventory.file_count,
        ruleset = manifest.ruleset.version,
        run_id = manifest.run.run_id
    )
}

fn render_findings_section(findings: &[Finding], tasks: &[EmittedTask]) -> String {
    if findings.is_empty() {
        return "## 2. Findings\n\nNo candidates detected on this snapshot. Absence of findings is NOT proof of absence of defects.\n".to_string();
    }
    let mut out = String::from(
        "## 2. Findings

Findings are CANDIDATES with evidence, not proven defects. Each carries
its detector's known false-positive/false-negative profile.
",
    );
    for (f, task) in findings.iter().zip(tasks.iter()) {
        let loc = &f.evidence_locations[0];
        out.push_str(&format!(
            "
### {id}

- Candidate statement: {statement}
- Rule: `{rule}` · Severity: {sev:?} · Confidence: {conf:.2}
- Detector: `{det}` (model_hint_unverified: {mhu})
- Evidence: `{file}:{ls}-{le}` — file snapshot BLAKE3 `{hash}`
- Why it matters: {why}
- Remediation readiness: **{ready}** (manual review required: {review})
- Analysis limitations:
",
            id = f.id,
            statement = f.candidate_statement,
            rule = f.rule_id,
            sev = f.severity,
            conf = f.provenance.confidence,
            det = f.provenance.detector,
            mhu = f.provenance.model_hint_unverified,
            file = loc.file,
            ls = loc.line_start,
            le = loc.line_end,
            hash = loc.file_blake3,
            why = why_it_matters(&f.rule_id),
            ready = task.readiness.readiness,
            review = task.readiness.criteria.manual_review_required
        ));
        for lim in &f.limitations {
            out.push_str(&format!("  - {lim}\n"));
        }
    }
    out
}

fn render_contracts_section(
    tasks: &[EmittedTask],
    contract_hashes: &BTreeMap<String, String>,
) -> String {
    let mut out = String::from(
        "## 3. Task contracts

A task contract is a PROPOSAL for the executor, never a command to
change code. The executor applies its own context/test/validation gates
and may honestly reject any contract.
",
    );
    if tasks.is_empty() {
        out.push_str("\nNo contracts emitted for this snapshot.\n");
        return out;
    }
    for t in tasks {
        let id = &t.contract.finding.id;
        let hash = contract_hashes.get(id).map(|s| s.as_str()).unwrap_or("-");
        out.push_str(&format!(
            "- `{id}` — contract BLAKE3 `{hash}` · readiness: {ready} · targets: {targets}\n",
            id = id,
            hash = hash,
            ready = t.readiness.readiness,
            targets = t.contract.target_files.join(", ")
        ));
    }
    out
}

const EXECUTOR_BOUNDARY_SECTION: &str = "## 4. Executor evidence boundary

- The analyzer fixes NOTHING. It only inventories, detects and proposes.
- The executor (deterministic kernel) treats every finding as untrusted:
  it verifies `context_before` of each patch hunk against the actual
  file, accepts only structured `apply_patch_v1` patches, runs real
  allowlisted tests (`run_tests_v1`) and can fail-closed reject the task.
- LLM output inside the executor is untrusted input: no shell, no
  filesystem, no network effects from model text — only kernel-owned
  primitives.
- Only a separate executor run can produce the `remediated` status,
  backed by event log, pre/post BLAKE3 hashes and `test_report_v1`.
";

const NEXT_STEP_SECTION: &str = "## 5. Next pilot step

1. Select ONE `remediation_ready` finding (requires an operator-provided
   reproducible failing test; otherwise the finding stays `candidate_only`).
2. Operator performs manual review of the finding and the proposed patch
   scope.
3. Run the executor in an ISOLATED workspace copy.
4. Preserve executor evidence: event log, patch pre/post BLAKE3 hashes,
   `test_report_v1`.
5. Independent re-verification outside the executor.
";

const HONEST_CLAIM_SECTION: &str = "## 6. Commercially honest claim

> The system produces a reproducible evidence trail from a read-only
> finding to a controlled remediation. It does not guarantee the absence
> of defects and does not replace human review.
";

fn render_report(
    manifest: &evidence_manifest::EvidenceManifest,
    findings: &[Finding],
    tasks: &[EmittedTask],
    contract_hashes: &BTreeMap<String, String>,
) -> String {
    let remediation_ready_count = tasks
        .iter()
        .filter(|t| t.readiness.readiness == READINESS_REMEDIATION_READY)
        .count();
    format!(
        "# Deterministic Remediation Pilot — Evidence Report

Analyzer version: {ver} · schema: {schema} · findings: {nf} · remediation-ready: {nr}

{scope}
{findings}
{contracts}
{boundary}
{next}
{claim}",
        ver = manifest.analyzer_version,
        schema = manifest.schema_version,
        nf = findings.len(),
        nr = remediation_ready_count,
        scope = render_scope_section(manifest),
        findings = render_findings_section(findings, tasks),
        contracts = render_contracts_section(tasks, contract_hashes),
        boundary = EXECUTOR_BOUNDARY_SECTION,
        next = NEXT_STEP_SECTION,
        claim = HONEST_CLAIM_SECTION
    )
}

/// Build the full deterministic bundle for a workspace. `repro_tests`
/// maps finding id → operator-provided reproducible test file path.
pub fn build_bundle(
    workspace: &Path,
    repro_tests: &BTreeMap<String, String>,
) -> Result<PilotBundle, String> {
    let canonical = workspace
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", workspace.display()))?;
    let ws_str = canonical.to_string_lossy().to_string();

    let inventory = ingestion::scan_workspace(&canonical)?;
    let candidates = scan_primitives::run_static_scan(&inventory, &canonical);
    let findings = triage::triage(&candidates, &inventory);
    let tasks =
        task_emitter::emit_tasks_with_readiness(&findings, &ws_str, &inventory, repro_tests);
    let manifest = evidence_manifest::build_manifest(&inventory, &ws_str);

    let mut contract_hashes = BTreeMap::new();
    for t in &tasks {
        let json = serde_json::to_string(&t.contract).expect("contract serialization");
        contract_hashes.insert(
            t.contract.finding.id.clone(),
            blake3::hash(json.as_bytes()).to_hex().to_string(),
        );
    }

    Ok(PilotBundle {
        manifest_json: evidence_manifest::manifest_json(&manifest),
        findings_json: serde_json::to_string_pretty(&findings).expect("findings serialization"),
        tasks_json: serde_json::to_string_pretty(&tasks).expect("tasks serialization"),
        report_md: render_report(&manifest, &findings, &tasks, &contract_hashes),
        manifest,
        findings,
        tasks,
        contract_hashes,
    })
}

/// Write the byte-stable artifacts into `out_dir` (never the workspace)
/// and return (file name, path, BLAKE3 of written bytes) sorted by name.
pub fn write_bundle_artifacts(
    bundle: &PilotBundle,
    out_dir: &Path,
) -> Result<Vec<(String, std::path::PathBuf, String)>, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("create {}: {e}", out_dir.display()))?;
    let items: [(&str, &str); 4] = [
        ("evidence_manifest_v1.json", &bundle.manifest_json),
        ("findings_v1.json", &bundle.findings_json),
        ("task_contracts_v0.json", &bundle.tasks_json),
        ("PILOT_REPORT.md", &bundle.report_md),
    ];
    let mut written = Vec::new();
    for (name, content) in items {
        let path = out_dir.join(name);
        std::fs::write(&path, content).map_err(|e| format!("write {}: {e}", path.display()))?;
        written.push((
            name.to_string(),
            path,
            blake3::hash(content.as_bytes()).to_hex().to_string(),
        ));
    }
    written.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(written)
}

/// Operator-facing run record for the operational log (non byte-stable
/// fields are filled by the caller).
pub fn make_run_record(
    run_id: &str,
    ts_unix: u64,
    workspace: &str,
    bundle: &PilotBundle,
    duration_ms: u64,
) -> RunRecord {
    RunRecord {
        run_id: run_id.to_string(),
        ts_unix,
        workspace: workspace.to_string(),
        files_scanned: bundle.manifest.inventory.file_count,
        candidates_found: bundle.findings.len(),
        findings_count: bundle.findings.len(),
        tasks_emitted: bundle
            .tasks
            .iter()
            .map(|t| t.contract.finding.id.clone())
            .collect(),
        analyzer_version: super::ANALYZER_VERSION.to_string(),
        duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pilot_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_fintech")
    }

    #[test]
    fn bundle_is_byte_stable_across_repeats() {
        let ws = pilot_fixture();
        let empty = BTreeMap::new();
        let a = build_bundle(&ws, &empty).unwrap();
        let b = build_bundle(&ws, &empty).unwrap();
        assert_eq!(a.manifest_json, b.manifest_json);
        assert_eq!(a.findings_json, b.findings_json);
        assert_eq!(a.tasks_json, b.tasks_json);
        assert_eq!(a.report_md, b.report_md);
        assert_eq!(a.contract_hashes, b.contract_hashes);
    }

    #[test]
    fn report_has_scope_and_no_forbidden_claims() {
        let ws = pilot_fixture();
        let bundle = build_bundle(&ws, &BTreeMap::new()).unwrap();
        let md = &bundle.report_md;
        assert!(md.starts_with("# Deterministic Remediation Pilot — Evidence Report"));
        assert!(md.contains("## 1. Scope and limitations"));
        assert!(md.contains("read-only candidate detection"));
        assert!(md.contains("Python-first"));
        for banned in FORBIDDEN_CLAIMS {
            assert!(
                !md.to_lowercase().contains(&banned.to_lowercase()),
                "forbidden claim present: {banned}"
            );
        }
    }

    #[test]
    fn pilot_fixture_detects_seeded_patterns_and_not_the_control() {
        let ws = pilot_fixture();
        let bundle = build_bundle(&ws, &BTreeMap::new()).unwrap();
        let rules: Vec<&str> = bundle.findings.iter().map(|f| f.rule_id.as_str()).collect();
        assert!(rules.contains(&"money-truncation"), "P1 missing: {rules:?}");
        assert!(rules.contains(&"money-round-bare"), "P2 missing: {rules:?}");
        assert!(rules.contains(&"todo-marker"), "P3 missing: {rules:?}");
        assert!(
            bundle
                .findings
                .iter()
                .all(|f| !f.evidence_locations[0].file.contains("safe_pricing")),
            "negative control must stay clean"
        );
        assert!(!bundle.findings.is_empty(), "fixture must produce findings");
    }
}
