//! Integration evidence tests for the v0.2 Enterprise Pilot package.
//!
//! Pure deterministic static analyzer coverage: NO LLM, NO MLX, NO model
//! mocks — there is no model in this pipeline at all.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use deterministic_ai_kernel::analyzer::pilot_report::{build_bundle, FORBIDDEN_CLAIMS};
use deterministic_ai_kernel::analyzer::task_emitter::READINESS_REMEDIATION_READY;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/pilot_fintech")
}

fn pilot_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/analyzer_pilot_report")
}

/// BLAKE3 of every file under `root` (sorted rel paths; deterministic).
fn workspace_fingerprint(root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "__pycache__" {
                    stack.push(p);
                }
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

fn copy_fixture(to: &Path) {
    let from = fixture();
    for (rel, _hash) in workspace_fingerprint(&from) {
        let src = from.join(&rel);
        let dst = to.join(&rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::copy(&src, &dst).unwrap();
    }
}

#[test]
fn scanned_fixture_hash_unchanged_by_report_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    copy_fixture(&ws);

    let before = workspace_fingerprint(&ws);
    let bundle = build_bundle(&ws, &BTreeMap::new(), &[]).unwrap();
    let after = workspace_fingerprint(&ws);

    assert_eq!(before, after, "analyzer must be read-only");
    assert_eq!(bundle.findings.len(), 3, "P1+P2+P3 expected");
}

#[test]
fn bundle_artifacts_are_byte_stable_and_honest() {
    let ws = fixture();
    let empty = BTreeMap::new();
    let a = build_bundle(&ws, &empty, &[]).unwrap();
    let b = build_bundle(&ws, &empty, &[]).unwrap();
    assert_eq!(a.manifest_json, b.manifest_json);
    assert_eq!(a.findings_json, b.findings_json);
    assert_eq!(a.tasks_json, b.tasks_json);
    assert_eq!(a.report_md, b.report_md);

    // Scope/limitations present; forbidden claims absent.
    assert!(a.report_md.contains("## 1. Scope and limitations"));
    assert!(a.report_md.contains("read-only candidate detection"));
    assert!(a.report_md.contains("Python-first"));
    assert!(a.report_md.contains("## 4. Executor evidence boundary"));
    assert!(a.report_md.contains("## 6. Commercially honest claim"));
    let md_lower = a.report_md.to_lowercase();
    for banned in FORBIDDEN_CLAIMS {
        assert!(
            !md_lower.contains(&banned.to_lowercase()),
            "forbidden claim: {banned}"
        );
    }

    // Manifest carries snapshot hash, ruleset version and no time fields.
    assert!(a.manifest_json.contains("\"workspace_snapshot_blake3\""));
    assert!(a.manifest_json.contains("python-fintech-rules/0.3.0"));
    for banned in ["started_at", "ts_unix", "duration_ms"] {
        assert!(!a.manifest_json.contains(banned));
    }
}

#[test]
fn readiness_promotion_requires_operator_provided_repro() {
    let ws = fixture();
    let mut repro = BTreeMap::new();
    repro.insert(
        "MONEY-TRUNCATION-LEDGER-15".to_string(),
        "test_ledger.py".to_string(),
    );
    let bundle = build_bundle(&ws, &repro, &[]).unwrap();

    let mut ready_ids = Vec::new();
    let mut candidate_only = 0usize;
    for t in &bundle.tasks {
        if t.readiness.readiness == READINESS_REMEDIATION_READY {
            ready_ids.push(t.contract.finding.id.clone());
            assert!(t.readiness.criteria.reproducible_test_present);
            assert!(t.readiness.criteria.target_file_snapshot_matches);
            assert!(t.readiness.criteria.evidence_complete);
        } else {
            candidate_only += 1;
        }
        assert!(
            t.readiness.criteria.manual_review_required,
            "manual review is never waivable"
        );
    }
    assert_eq!(ready_ids, vec!["MONEY-TRUNCATION-LEDGER-15"]);
    assert_eq!(candidate_only, 2);

    // Without operator input everything is candidate_only.
    let plain = build_bundle(&ws, &BTreeMap::new(), &[]).unwrap();
    assert!(plain
        .tasks
        .iter()
        .all(|t| t.readiness.readiness != READINESS_REMEDIATION_READY));
}

#[test]
fn cli_double_run_is_byte_identical_and_workspace_untouched() {
    let bin = pilot_bin();
    assert!(bin.exists(), "build the bins first: cargo test builds them");

    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    copy_fixture(&ws);
    let before = workspace_fingerprint(&ws);

    let mut outputs = Vec::new();
    for run in ["run1", "run2"] {
        let out = tmp.path().join(run);
        let status = std::process::Command::new(&bin)
            .arg("--workspace")
            .arg(&ws)
            .arg("--output")
            .arg(&out)
            .status()
            .expect("spawn analyzer_pilot_report");
        assert!(status.success(), "pilot report run {run} failed");

        let names: BTreeSet<String> = std::fs::read_dir(&out)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "PILOT_REPORT.md",
                "audit_log_v1.json",
                "evidence_manifest_v1.json",
                "findings_v1.json",
                "task_contracts_v0.json",
            ]
            .into_iter()
            .map(|s| s.to_string())
            .collect::<BTreeSet<_>>(),
            "full evidence package expected"
        );
        outputs.push(out);
    }

    // Deterministic artifacts byte-identical across runs.
    for name in [
        "evidence_manifest_v1.json",
        "findings_v1.json",
        "task_contracts_v0.json",
        "PILOT_REPORT.md",
    ] {
        let a = std::fs::read(outputs[0].join(name)).unwrap();
        let b = std::fs::read(outputs[1].join(name)).unwrap();
        assert_eq!(a, b, "{name} must be byte-stable across runs");
    }

    // Findings count and IDs stable (parse run1 findings).
    let findings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(outputs[0].join("findings_v1.json")).unwrap(),
    )
    .unwrap();
    let ids: Vec<&str> = findings
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "MONEY-TRUNCATION-LEDGER-15",
            "MONEY-ROUND-BARE-LEDGER-20",
            "TODO-MARKER-LEDGER-23",
        ]
    );

    // Scanned workspace untouched.
    let after = workspace_fingerprint(&ws);
    assert_eq!(before, after, "CLI must not modify the workspace");
}

#[test]
fn cli_external_sast_run_is_byte_stable_and_merges_findings() {
    let bin = pilot_bin();
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    copy_fixture(&ws);
    let before = workspace_fingerprint(&ws);

    let semgrep = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("analyzer_examples/external_reports/semgrep_sample.json");
    let bandit = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("analyzer_examples/external_reports/bandit_sample.json");

    let mut outputs = Vec::new();
    for run in ["ext1", "ext2"] {
        let out = tmp.path().join(run);
        let status = std::process::Command::new(&bin)
            .arg("--workspace")
            .arg(&ws)
            .arg("--output")
            .arg(&out)
            .arg("--external-sast")
            .arg(&semgrep)
            .arg("--external-sast")
            .arg(&bandit)
            .status()
            .expect("spawn analyzer_pilot_report");
        assert!(status.success(), "external run {run} failed");
        outputs.push(out);
    }

    for name in [
        "evidence_manifest_v1.json",
        "findings_v1.json",
        "task_contracts_v0.json",
        "PILOT_REPORT.md",
    ] {
        let a = std::fs::read(outputs[0].join(name)).unwrap();
        let b = std::fs::read(outputs[1].join(name)).unwrap();
        assert_eq!(a, b, "{name} must be byte-stable with externals");
    }

    // 3 static + 2 external findings; external detectors present.
    let findings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(outputs[0].join("findings_v1.json")).unwrap(),
    )
    .unwrap();
    let arr = findings.as_array().unwrap();
    assert_eq!(arr.len(), 5, "3 static + 2 external expected");
    let external_count = arr
        .iter()
        .filter(|f| f["provenance"]["detector"] == "external")
        .count();
    assert_eq!(external_count, 2);

    // Manifest records both sources and the traversal rejection.
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(outputs[0].join("evidence_manifest_v1.json")).unwrap(),
    )
    .unwrap();
    let sources = manifest["external_sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    let rejected: u64 = sources
        .iter()
        .map(|s| s["rejected_paths"].as_u64().unwrap())
        .sum();
    assert_eq!(rejected, 1, "semgrep traversal probe rejected");
    for s in sources {
        assert_eq!(s["report_blake3"].as_str().unwrap().len(), 64);
    }

    // Workspace untouched even with external inputs.
    let after = workspace_fingerprint(&ws);
    assert_eq!(before, after);
}

#[test]
fn cli_external_sast_unknown_format_fails_closed() {
    let bin = pilot_bin();
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    copy_fixture(&ws);
    let bad = tmp.path().join("bad_report.json");
    std::fs::write(&bad, r#"{"results": [{"weird": true}]}"#).unwrap();

    let status = std::process::Command::new(&bin)
        .arg("--workspace")
        .arg(&ws)
        .arg("--output")
        .arg(tmp.path().join("out"))
        .arg("--external-sast")
        .arg(&bad)
        .status()
        .expect("spawn");
    assert!(!status.success(), "unknown SAST format must fail closed");
}

#[test]
fn cli_refuses_output_inside_workspace() {
    let bin = pilot_bin();
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    copy_fixture(&ws);

    let status = std::process::Command::new(&bin)
        .arg("--workspace")
        .arg(&ws)
        .arg("--output")
        .arg(ws.join("inside_out"))
        .status()
        .expect("spawn");
    assert!(!status.success(), "must refuse writes into the workspace");
}
