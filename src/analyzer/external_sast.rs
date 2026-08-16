//! External SAST ingestion (v0.3) — Semgrep/Bandit JSON reports as a
//! READ-ONLY, UNTRUSTED candidate source.
//!
//! Invariants:
//! - The analyzer never EXECUTES external tools; operators run them
//!   separately and hand over the report file. Ingestion is pure file
//!   reading + deterministic normalization.
//! - External candidates flow through the same dedup → triage → emitter
//!   path as static ones; the executor still does not trust findings.
//! - No effects, no timestamps carried into candidates: report time
//!   metadata is deliberately ignored so identical findings normalize
//!   byte-identically regardless of when the tool ran.
//! - Path safety: any reported path is resolved against the workspace;
//!   paths escaping it (e.g. `../`) are rejected and counted, never
//!   followed.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use super::scan_primitives::{Candidate, DETECTOR_EXTERNAL};

pub const DETECTOR_SEMGREP: &str = "semgrep";
pub const DETECTOR_BANDIT: &str = "bandit";

/// Result of ingesting one external report.
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSummary {
    /// `semgrep` | `bandit` (tool inferred from the report structure).
    pub tool: String,
    /// BLAKE3 of the raw report bytes — the audit anchor proving which
    /// external input produced these candidates.
    pub report_blake3: String,
    pub candidates: Vec<Candidate>,
    /// Reported paths rejected because they escape the workspace.
    pub rejected_paths: usize,
}

/// Lexically normalize `raw` against `workspace`; `None` when the result
/// escapes the workspace (path-traversal guard). Never touches the
/// filesystem.
fn normalize_to_workspace(workspace: &Path, raw: &str) -> Option<String> {
    let p = Path::new(raw);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        workspace.join(p)
    };
    let mut norm = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::ParentDir => {
                norm.pop();
            }
            Component::CurDir => {}
            other => norm.push(other.as_os_str()),
        }
    }
    let rel = norm.strip_prefix(workspace).ok()?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    Some(rel.to_string_lossy().replace('\\', "/"))
}

fn cap(s: &str, n: usize) -> String {
    s.trim().chars().take(n).collect()
}

// Severity→confidence classification for external candidates lives in
// `triage::classify_external` (single source of truth); ingestion only
// carries the tool-reported severity string through.

/// Parse one Semgrep `--json` report. Only deterministic fields are
/// consumed: check_id, path, start/end lines, message, severity, lines.
fn normalize_segrep(results: &[Value], workspace: &Path) -> (Vec<Candidate>, usize) {
    let mut out = Vec::new();
    let mut rejected = 0usize;
    for r in results {
        let Some(check_id) = r.get("check_id").and_then(|v| v.as_str()) else {
            rejected += 1;
            continue;
        };
        let Some(raw_path) = r.get("path").and_then(|v| v.as_str()) else {
            rejected += 1;
            continue;
        };
        let Some(file) = normalize_to_workspace(workspace, raw_path) else {
            rejected += 1;
            continue;
        };
        let line_start = r
            .get("start")
            .and_then(|s| s.get("line"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1) as usize;
        let line_end = r
            .get("end")
            .and_then(|s| s.get("line"))
            .and_then(|v| v.as_u64())
            .unwrap_or(line_start as u64) as usize;
        let extra = r.get("extra");
        let message = extra
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("(no message)");
        let severity = extra
            .and_then(|e| e.get("severity"))
            .and_then(|v| v.as_str())
            .unwrap_or("INFO");
        let snippet = extra
            .and_then(|e| e.get("lines"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("");
        out.push(Candidate {
            rule_id: format!("semgrep:{check_id}"),
            file,
            line_start: line_start.max(1),
            line_end: line_end.max(line_start.max(1)),
            suspicion: cap(message, 200),
            snippet: cap(snippet, 160),
            // Provenance category; the tool name lives in the rule_id
            // prefix (`semgrep:`).
            detector: DETECTOR_EXTERNAL.to_string(),
            external_severity: Some(severity.to_uppercase()),
        });
    }
    (out, rejected)
}

/// Parse one Bandit `-f json` report. Deterministic fields only:
/// test_id, filename, line_number, issue_text, issue_severity,
/// issue_confidence, code.
fn normalize_bandit(results: &[Value], workspace: &Path) -> (Vec<Candidate>, usize) {
    let mut out = Vec::new();
    let mut rejected = 0usize;
    for r in results {
        let Some(test_id) = r.get("test_id").and_then(|v| v.as_str()) else {
            rejected += 1;
            continue;
        };
        let Some(raw_path) = r.get("filename").and_then(|v| v.as_str()) else {
            rejected += 1;
            continue;
        };
        let Some(file) = normalize_to_workspace(workspace, raw_path) else {
            rejected += 1;
            continue;
        };
        let line = r.get("line_number").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
        let message = r
            .get("issue_text")
            .and_then(|v| v.as_str())
            .unwrap_or("(no message)");
        let severity = r
            .get("issue_severity")
            .and_then(|v| v.as_str())
            .unwrap_or("LOW");
        let snippet = r
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("");
        out.push(Candidate {
            rule_id: format!("bandit:{test_id}"),
            file,
            line_start: line.max(1),
            line_end: line.max(1),
            suspicion: cap(message, 200),
            snippet: cap(snippet, 160),
            // Provenance category; the tool name lives in the rule_id
            // prefix (`bandit:`).
            detector: DETECTOR_EXTERNAL.to_string(),
            external_severity: Some(severity.to_uppercase()),
        });
    }
    (out, rejected)
}

/// Ingest one external SAST report (read-only). Format is auto-detected
/// from structure: `results[].check_id` → Semgrep, `results[].test_id`
/// → Bandit. Unknown formats fail closed.
pub fn parse_external_report(path: &Path, workspace: &Path) -> Result<ExternalSummary, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let report_blake3 = blake3::hash(&bytes).to_hex().to_string();
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "external report is not valid JSON ({}): {e}",
            path.display()
        )
    })?;
    let results = value
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            format!(
                "unrecognized external SAST format in {}: no `results` array",
                path.display()
            )
        })?;

    let first = results.first();
    let tool = if first.and_then(|r| r.get("check_id")).is_some() {
        DETECTOR_SEMGREP
    } else if first.and_then(|r| r.get("test_id")).is_some() {
        DETECTOR_BANDIT
    } else if results.is_empty() {
        // Empty report: infer from top-level fingerprints, else unknown.
        if value.get("generated_at").is_some() {
            DETECTOR_BANDIT
        } else if value.get("time").is_some() || value.get("paths").is_some() {
            DETECTOR_SEMGREP
        } else {
            return Err(format!(
                "unrecognized external SAST format in {}: empty results without tool fingerprint",
                path.display()
            ));
        }
    } else {
        return Err(format!(
            "unrecognized external SAST format in {}: results lack check_id/test_id",
            path.display()
        ));
    };

    let (mut candidates, rejected_paths) = match tool {
        DETECTOR_SEMGREP => normalize_segrep(results, workspace),
        _ => normalize_bandit(results, workspace),
    };
    // Deterministic order regardless of report order.
    candidates.sort_by(|a, b| {
        a.rule_id
            .cmp(&b.rule_id)
            .then(a.file.cmp(&b.file))
            .then(a.line_start.cmp(&b.line_start))
            .then(a.line_end.cmp(&b.line_end))
    });
    Ok(ExternalSummary {
        tool: tool.to_string(),
        report_blake3,
        candidates,
        rejected_paths,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SEMGREP_SAMPLE: &str = r#"{
      "version": "1.99.0",
      "time": {"total_bytes": 1, "profiling_time": 0.5},
      "paths": {"scanned": ["billing/ledger.py"]},
      "results": [
        {
          "check_id": "python.fintech.money-truncation-int",
          "path": "billing/ledger.py",
          "start": {"line": 15, "col": 12, "offset": 300},
          "end": {"line": 15, "col": 34, "offset": 322},
          "extra": {
            "message": "int() truncates monetary amounts toward zero",
            "severity": "ERROR",
            "lines": "    return int(amount * 100) / 100"
          }
        },
        {
          "check_id": "python.fintech.escape-attempt",
          "path": "../../etc/passwd",
          "start": {"line": 1},
          "end": {"line": 1},
          "extra": {"message": "must be rejected", "severity": "INFO"}
        }
      ]
    }"#;

    const BANDIT_SAMPLE: &str = r#"{
      "errors": [],
      "generated_at": "2026-08-16T00:00:00Z",
      "metrics": {"_totals": {"SEVERITY.HIGH": 1}},
      "results": [
        {
          "test_id": "B700",
          "test_name": "fintech_round_no_policy",
          "filename": "billing/ledger.py",
          "line_number": 20,
          "issue_text": "round() without explicit rounding policy on monetary path",
          "issue_severity": "MEDIUM",
          "issue_confidence": "HIGH",
          "code": "    return round(value)\n"
        }
      ]
    }"#;

    fn write_tmp(name: &str, content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        // File name is informational; keep the requested stem for clarity.
        let _ = name;
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn semgrep_report_normalizes_deterministically_and_rejects_traversal() {
        let ws = Path::new("/workspace");
        let f = write_tmp("semgrep.json", SEMGREP_SAMPLE);
        let a = parse_external_report(f.path(), ws).unwrap();
        let b = parse_external_report(f.path(), ws).unwrap();
        assert_eq!(a, b, "normalization must be deterministic");
        assert_eq!(a.tool, "semgrep");
        assert_eq!(a.candidates.len(), 1, "traversal result must be dropped");
        assert_eq!(a.rejected_paths, 1);
        let c = &a.candidates[0];
        assert_eq!(c.rule_id, "semgrep:python.fintech.money-truncation-int");
        assert_eq!(c.file, "billing/ledger.py");
        assert_eq!((c.line_start, c.line_end), (15, 15));
        assert_eq!(c.detector, "external", "provenance category expected");
        assert_eq!(c.external_severity.as_deref(), Some("ERROR"));
        assert_eq!(a.report_blake3.len(), 64);
    }

    #[test]
    fn bandit_report_normalizes_deterministically() {
        let ws = Path::new("/workspace");
        let f = write_tmp("bandit.json", BANDIT_SAMPLE);
        let s = parse_external_report(f.path(), ws).unwrap();
        assert_eq!(s.tool, "bandit");
        assert_eq!(s.candidates.len(), 1);
        assert_eq!(s.rejected_paths, 0);
        let c = &s.candidates[0];
        assert_eq!(c.rule_id, "bandit:B700");
        assert_eq!(c.file, "billing/ledger.py");
        assert_eq!(c.line_start, 20);
        assert_eq!(c.detector, "external", "provenance category expected");
        assert_eq!(c.external_severity.as_deref(), Some("MEDIUM"));
    }

    #[test]
    fn report_timestamps_do_not_affect_output() {
        let ws = Path::new("/workspace");
        let v1 = SEMGREP_SAMPLE.replace("1.99.0", "1.0.0");
        let f1 = write_tmp("a.json", &v1);
        let f2 = write_tmp("b.json", SEMGREP_SAMPLE);
        let a = parse_external_report(f1.path(), ws).unwrap();
        let b = parse_external_report(f2.path(), ws).unwrap();
        assert_eq!(a.candidates, b.candidates, "only findings matter");
        assert_ne!(a.report_blake3, b.report_blake3, "raw bytes still distinct");
    }

    #[test]
    fn unknown_format_fails_closed() {
        let ws = Path::new("/workspace");
        let f = write_tmp("x.json", r#"{"results": [{"weird": true}]}"#);
        assert!(parse_external_report(f.path(), ws).is_err());
        let f2 = write_tmp("y.json", "not json at all");
        assert!(parse_external_report(f2.path(), ws).is_err());
        let f3 = write_tmp("z.json", r#"{"results": []}"#);
        assert!(
            parse_external_report(f3.path(), ws).is_err(),
            "empty results without tool fingerprint must fail closed"
        );
    }

    #[test]
    fn path_normalization_guards() {
        let ws = Path::new("/workspace");
        assert_eq!(
            normalize_to_workspace(ws, "billing/f.py").as_deref(),
            Some("billing/f.py")
        );
        assert_eq!(
            normalize_to_workspace(ws, "/workspace/billing/f.py").as_deref(),
            Some("billing/f.py")
        );
        assert!(normalize_to_workspace(ws, "../outside.py").is_none());
        assert!(normalize_to_workspace(ws, "billing/../../outside.py").is_none());
        assert!(normalize_to_workspace(ws, "/etc/passwd").is_none());
    }
}
