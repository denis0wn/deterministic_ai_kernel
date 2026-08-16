//! Scan primitives — deterministic static heuristics that surface
//! defect CANDIDATES.
//!
//! Candidates are untrusted hints: they never become effects. The rules
//! are intentionally minimal SAST-like patterns; richer detection (model
//! pass, external SAST) plugs in later behind the same Candidate shape
//! (provenance.detector distinguishes the source).
//!
//! Pilot financial ruleset (Python-first, documented in
//! docs/ANALYZER_RULES_CATALOG.md): `money-truncation`,
//! `money-round-bare`, `offbyone-range`, `none-arith`, `todo-marker`
//! (finance-path scoped). `dangerous-eval` is a general safety rule kept
//! alongside the financial set. These rules do NOT claim to find all
//! financial defects — see per-rule limitations in `rule_limitations`.

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::ingestion::WorkspaceInventory;

/// Version of the static ruleset — recorded in the evidence manifest so
/// every finding is traceable to exact rule semantics.
pub const RULESET_VERSION: &str = "python-fintech-rules/0.2.0";

/// The five financial pilot rules (client-explainable, documented).
pub const FINANCIAL_RULE_IDS: &[&str] = &[
    "money-truncation",
    "money-round-bare",
    "none-arith",
    "offbyone-range",
    "todo-marker",
];

/// All rule ids executed by this scanner version, sorted (deterministic).
pub fn ruleset_ids() -> Vec<String> {
    let mut ids: Vec<String> = RULES.iter().map(|r| r.id.to_string()).collect();
    ids.sort();
    ids
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// Stable rule identifier, e.g. `money-truncation`.
    pub rule_id: String,
    /// Workspace-relative file.
    pub file: String,
    /// 1-based inclusive line range of the suspicion.
    pub line_start: usize,
    pub line_end: usize,
    /// Human-readable suspicion (short).
    pub suspicion: String,
    /// The offending source line (trimmed, capped).
    pub snippet: String,
}

struct Rule {
    id: &'static str,
    /// Line predicate: (file rel path, all lines, current index).
    check: fn(&str, &[&str], usize) -> Option<&'static str>,
}

/// Path keywords marking money/risk/limit/fee/settlement domain files for
/// path-scoped rules (`todo-marker`, `money-round-bare`).
pub fn is_finance_path(file: &str) -> bool {
    let lower = file.to_lowercase();
    [
        "billing",
        "fee",
        "payment",
        "charge",
        "price",
        "pricing",
        "invoice",
        "money",
        "risk",
        "limit",
        "settlement",
        "tax",
        "discount",
    ]
    .iter()
    .any(|k| lower.contains(k))
}

fn is_noise_line(t: &str) -> bool {
    t.starts_with('#') || t.starts_with('"')
}

fn check_money_truncation(_file: &str, lines: &[&str], idx: usize) -> Option<&'static str> {
    let line = lines[idx];
    if is_noise_line(line.trim()) {
        return None;
    }
    if line.contains("int(") && line.contains("* 100") {
        return Some("money amount truncated via int() — sub-cent fractions lost");
    }
    None
}

fn check_bare_round(file: &str, lines: &[&str], idx: usize) -> Option<&'static str> {
    let line = lines[idx];
    if is_noise_line(line.trim()) {
        return None;
    }
    if !is_finance_path(file) {
        return None; // scoped: bare round() only matters on monetary paths
    }
    // round(x) without an explicit ndigits / policy argument. Only a
    // real call counts: the preceding character must not be part of an
    // identifier (so `settlement_round(` or `around(` do not match).
    for (pos, _) in line.match_indices("round(") {
        if pos > 0 {
            let prev = line[..pos].chars().next_back().unwrap();
            if prev.is_alphanumeric() || prev == '_' {
                continue;
            }
        }
        let after = &line[pos + 6..];
        let first_arg = after.split(')').next().unwrap_or("");
        if !first_arg.is_empty() && !first_arg.contains(',') {
            return Some(
                "bare round() without explicit rounding policy — banker's-rounding surprises on money",
            );
        }
    }
    None
}

fn check_offbyone_range(_file: &str, lines: &[&str], idx: usize) -> Option<&'static str> {
    let line = lines[idx];
    if is_noise_line(line.trim()) {
        return None;
    }
    if line.contains("range(1, len(") || line.contains("range(1,len(") {
        return Some("range starts at 1 over len() — first element may be skipped");
    }
    None
}

/// D3-class defect: `x = obj.get("k")` WITHOUT a default, then `x` used
/// in arithmetic within the next two lines — possible None crash.
fn check_none_arithmetic(_file: &str, lines: &[&str], idx: usize) -> Option<&'static str> {
    let line = lines[idx];
    if is_noise_line(line.trim()) {
        return None;
    }
    let get_pos = line.find(".get(")?;
    // Extract the assigned variable: `<var> = ...get(...)`
    let before = &line[..get_pos];
    let eq_pos = before.rfind('=')?;
    let var = before[..eq_pos]
        .trim()
        .split('.')
        .next_back()
        .unwrap_or("")
        .trim();
    if var.is_empty() || !var.chars().next().is_some_and(|c| c.is_alphabetic()) {
        return None;
    }
    // Single-argument .get( → no default provided.
    let args = line[get_pos + 5..].split(')').next().unwrap_or("");
    if args.contains(',') {
        return None; // has a default value
    }
    // Lookahead: arithmetic on the variable within the next two lines.
    let arith = [" * ", " + ", " - ", " / ", "* ", " *", "- "];
    for ahead in 1..=2 {
        if let Some(next) = lines.get(idx + ahead) {
            for op in arith {
                if next.contains(&format!("{var}{op}")) || next.contains(&format!("{op}{var}")) {
                    return Some(
                        "arithmetic on dict.get() result without default — possible None crash",
                    );
                }
            }
        }
    }
    None
}

/// TODO/FIXME scoped to money/risk/limit/fee/settlement paths — a marker
/// in generic code is noise; a marker on a financial path is audit risk.
fn check_todo_marker(file: &str, lines: &[&str], idx: usize) -> Option<&'static str> {
    if !is_finance_path(file) {
        return None;
    }
    let line = lines[idx];
    if line.contains("TODO") || line.contains("FIXME") {
        return Some("TODO/FIXME marker left in a money/risk/limit/fee/settlement path");
    }
    None
}

fn check_dangerous_eval(_file: &str, lines: &[&str], idx: usize) -> Option<&'static str> {
    let line = lines[idx];
    if is_noise_line(line.trim()) {
        return None;
    }
    if line.contains("eval(") || line.contains("exec(") {
        return Some("eval()/exec() — arbitrary code execution risk");
    }
    None
}

const RULES: &[Rule] = &[
    Rule {
        id: "money-truncation",
        check: check_money_truncation,
    },
    Rule {
        id: "money-round-bare",
        check: check_bare_round,
    },
    Rule {
        id: "offbyone-range",
        check: check_offbyone_range,
    },
    Rule {
        id: "none-arith",
        check: check_none_arithmetic,
    },
    Rule {
        id: "dangerous-eval",
        check: check_dangerous_eval,
    },
    Rule {
        id: "todo-marker",
        check: check_todo_marker,
    },
];

/// Known analysis limitations per rule (honest FP/FN disclosure carried
/// into every finding; see docs/ANALYZER_RULES_CATALOG.md).
pub fn rule_limitations(rule_id: &str) -> Vec<String> {
    match rule_id {
        "money-truncation" => vec![
            "false positives: int(x * 100) used for non-monetary scaling (percent formatting, basis points display)".to_string(),
            "false negatives: truncation via // operator, math.floor, format specs, or Decimal(int(...)) is not matched".to_string(),
        ],
        "money-round-bare" => vec![
            "false positives: round() on non-monetary values inside a finance-named file".to_string(),
            "false negatives: rounding hidden in format()/f-strings, numpy, or Decimal local contexts is not matched".to_string(),
        ],
        "offbyone-range" => vec![
            "false positives: range(1, len(...)) that intentionally skips element 0 (e.g. diff against previous element)".to_string(),
            "false negatives: off-by-one errors in while-loops, slices, or inclusive/exclusive bound mismatches are not matched".to_string(),
        ],
        "none-arith" => vec![
            "false positives: .get() result validated for None between assignment and use".to_string(),
            "false negatives: arithmetic further than 2 lines away, chained optional access, or missing nested keys".to_string(),
        ],
        "todo-marker" => vec![
            "false positives: informational comments mentioning TODO without unfinished work".to_string(),
            "false negatives: alternate spellings (To Do, XXX, HACK) are not matched".to_string(),
        ],
        "dangerous-eval" => vec![
            "false positives: ast.literal_eval(...) matches the eval( substring but is not arbitrary execution".to_string(),
            "false negatives: dynamic imports or getattr-dispatched calls are not matched".to_string(),
        ],
        _ => vec!["no limitation profile registered for this rule".to_string()],
    }
}

/// Languages the static rules apply to today (Python-first pilot scope —
/// JVM/Go/JS are explicitly backlog, not supported).
fn scannable(language: &str) -> bool {
    matches!(language, "python")
}

/// Read-only static scan over the inventory. Deterministic: files are
/// processed in sorted rel_path order, lines top-down, rules in RULES
/// order.
pub fn run_static_scan(inv: &WorkspaceInventory, root: &Path) -> Vec<Candidate> {
    let mut out = Vec::new();
    for entry in &inv.files {
        if !scannable(&entry.language) {
            continue;
        }
        let full = root.join(&entry.rel_path);
        let Ok(content) = std::fs::read_to_string(&full) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            for rule in RULES {
                if let Some(suspicion) = (rule.check)(&entry.rel_path, &lines, idx) {
                    out.push(Candidate {
                        rule_id: rule.id.to_string(),
                        file: entry.rel_path.clone(),
                        line_start: idx + 1,
                        line_end: idx + 1,
                        suspicion: suspicion.to_string(),
                        snippet: line.trim().chars().take(160).collect(),
                    });
                }
            }
        }
    }
    dedupe_candidates(out)
}

/// Whitespace-normalized evidence text used for deduplication.
fn normalized_snippet(c: &Candidate) -> String {
    c.snippet.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Deterministic dedup key: rule + location + normalized evidence.
fn dedup_key(c: &Candidate) -> (String, String, usize, usize, String) {
    (
        c.rule_id.clone(),
        c.file.clone(),
        c.line_start,
        c.line_end,
        normalized_snippet(c),
    )
}

/// Deterministic deduplication, independent of input order: identical
/// (rule_id, location, normalized evidence) tuples collapse to one
/// candidate. Output is sorted by the dedup key with no further
/// tie-breaker ambiguity.
pub fn dedupe_candidates(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by_key(dedup_key);
    let mut out: Vec<Candidate> = Vec::new();
    for cand in candidates {
        match out.last() {
            Some(prev) if dedup_key(prev) == dedup_key(&cand) => {}
            _ => out.push(cand),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::ingestion::scan_workspace;
    use std::path::PathBuf;

    fn toy() -> (WorkspaceInventory, PathBuf) {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/billing_python");
        (scan_workspace(&root).unwrap(), root)
    }

    /// Write `files` (rel path -> content) into a fresh temp workspace.
    fn temp_workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, content) in files {
            let path = dir.path().join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, content).unwrap();
        }
        dir
    }

    fn scan_dir(dir: &Path) -> Vec<Candidate> {
        let inv = scan_workspace(dir).unwrap();
        run_static_scan(&inv, dir)
    }

    fn rule_hits<'a>(cands: &'a [Candidate], rule: &str) -> Vec<&'a Candidate> {
        cands.iter().filter(|c| c.rule_id == rule).collect()
    }

    #[test]
    fn static_scan_finds_all_three_seeded_defects() {
        let (inv, root) = toy();
        let cands = run_static_scan(&inv, &root);
        let rules: Vec<&str> = cands.iter().map(|c| c.rule_id.as_str()).collect();
        assert!(rules.contains(&"money-truncation"), "D1 missing: {rules:?}");
        assert!(rules.contains(&"offbyone-range"), "D2 missing: {rules:?}");
        assert!(rules.contains(&"none-arith"), "D3 missing: {rules:?}");
    }

    #[test]
    fn static_scan_is_deterministic() {
        let (inv, root) = toy();
        assert_eq!(run_static_scan(&inv, &root), run_static_scan(&inv, &root));
    }

    #[test]
    fn candidates_carry_evidence_coordinates() {
        let (inv, root) = toy();
        let cands = run_static_scan(&inv, &root);
        for c in &cands {
            assert!(c.line_start >= 1);
            assert!(c.line_end >= c.line_start);
            assert!(!c.snippet.is_empty(), "empty snippet for {:?}", c);
        }
    }

    // --- A3: positive + negative test for every financial rule ---

    #[test]
    fn rule_money_truncation_positive_and_negative() {
        let dir = temp_workspace(&[(
            "billing/charges.py",
            "def to_cents(amount):\n    cents = int(amount * 100)\n    return cents\n\ndef scale_display(x):\n    # int(x * 100) inside a comment must not fire\n    return x\n",
        )]);
        let cands = scan_dir(dir.path());
        let hits = rule_hits(&cands, "money-truncation");
        assert_eq!(hits.len(), 1, "exactly the live truncation: {cands:?}");
        assert_eq!(hits[0].line_start, 2);
        // Negative: no int() truncation at all -> no hits.
        let clean = temp_workspace(&[("billing/charges.py", "def f(a):\n    return a + 1\n")]);
        assert!(rule_hits(&scan_dir(clean.path()), "money-truncation").is_empty());
    }

    #[test]
    fn rule_money_round_bare_positive_and_negative() {
        let dir = temp_workspace(&[(
            "pricing/rounding.py",
            "def settlement_round(value):\n    return round(value)\n\ndef fee2(x):\n    return round(x, 2)\n",
        )]);
        let cands = scan_dir(dir.path());
        let hits = rule_hits(&cands, "money-round-bare");
        // The real call fires; the `settlement_round(` name and the
        // explicit-arg round(x, 2) do not.
        assert_eq!(hits.len(), 1, "only the no-policy round() call: {cands:?}");
        assert_eq!(hits[0].line_start, 2);
        // Negative: bare round() OUTSIDE a finance path must not fire.
        let nonfin = temp_workspace(&[("util/misc.py", "def g(x):\n    return round(x)\n")]);
        assert!(rule_hits(&scan_dir(nonfin.path()), "money-round-bare").is_empty());
    }

    #[test]
    fn rule_offbyone_range_positive_and_negative() {
        let dir = temp_workspace(&[(
            "limits/tiers.py",
            "def walk(tiers):\n    for i in range(1, len(tiers)):\n        pass\n",
        )]);
        let cands = scan_dir(dir.path());
        let hits = rule_hits(&cands, "offbyone-range");
        assert_eq!(hits.len(), 1, "{cands:?}");
        assert_eq!(hits[0].line_start, 2);
        let clean = temp_workspace(&[(
            "limits/tiers.py",
            "def walk(t):\n    for i in range(len(t)):\n        pass\n",
        )]);
        assert!(rule_hits(&scan_dir(clean.path()), "offbyone-range").is_empty());
    }

    #[test]
    fn rule_none_arith_positive_and_negative() {
        let dir = temp_workspace(&[(
            "settlement/net.py",
            "def net(amount, cust):\n    d = cust.get(\"discount\")\n    return amount - amount * d\n",
        )]);
        let cands = scan_dir(dir.path());
        let hits = rule_hits(&cands, "none-arith");
        assert_eq!(hits.len(), 1, "{cands:?}");
        assert_eq!(hits[0].line_start, 2);
        // Negative: .get() WITH a default must not fire.
        let safe = temp_workspace(&[(
            "settlement/net.py",
            "def net(amount, cust):\n    d = cust.get(\"discount\", 0.0)\n    return amount - amount * d\n",
        )]);
        assert!(rule_hits(&scan_dir(safe.path()), "none-arith").is_empty());
    }

    #[test]
    fn rule_todo_marker_scoped_to_finance_paths() {
        let dir = temp_workspace(&[
            (
                "risk/limits.py",
                "# TODO: rework limit check\ndef f(x):\n    return x\n",
            ),
            (
                "util/helpers.py",
                "# TODO: refactor later\ndef g(x):\n    return x\n",
            ),
        ]);
        let cands = scan_dir(dir.path());
        let hits = rule_hits(&cands, "todo-marker");
        assert_eq!(hits.len(), 1, "only the finance-path TODO fires: {cands:?}");
        assert_eq!(hits[0].file, "risk/limits.py");
    }

    // --- A2: deduplication ---

    fn cand(rule: &str, file: &str, line: usize, snippet: &str) -> Candidate {
        Candidate {
            rule_id: rule.to_string(),
            file: file.to_string(),
            line_start: line,
            line_end: line,
            suspicion: "s".to_string(),
            snippet: snippet.to_string(),
        }
    }

    #[test]
    fn dedup_collapses_identical_evidence_regardless_of_input_order() {
        let a = cand("money-truncation", "billing/f.py", 10, "x = int(v * 100)");
        let b = cand("money-truncation", "billing/f.py", 10, "x = int(v * 100)");
        let c = cand("none-arith", "billing/f.py", 12, "y = d.get(\"k\")");
        let order1 = dedupe_candidates(vec![a.clone(), b.clone(), c.clone()]);
        let order2 = dedupe_candidates(vec![c.clone(), b.clone(), a.clone()]);
        assert_eq!(order1, order2, "input order must not matter");
        assert_eq!(order1.len(), 2, "duplicates collapsed exactly once");
    }

    #[test]
    fn dedup_normalizes_whitespace_in_evidence() {
        let a = cand("money-truncation", "billing/f.py", 10, "x = int(v * 100)");
        let b = cand(
            "money-truncation",
            "billing/f.py",
            10,
            "x =  int(v  *  100)",
        );
        assert_eq!(dedupe_candidates(vec![a, b]).len(), 1);
    }

    #[test]
    fn dedup_keeps_distinct_locations_and_rules() {
        let a = cand("money-truncation", "billing/f.py", 10, "x = int(v * 100)");
        let b = cand("money-truncation", "billing/f.py", 11, "x = int(v * 100)");
        let c = cand("money-round-bare", "billing/f.py", 10, "x = int(v * 100)");
        assert_eq!(dedupe_candidates(vec![a, b, c]).len(), 3);
    }

    #[test]
    fn ruleset_metadata_is_stable_and_documented() {
        let ids = ruleset_ids();
        assert_eq!(ids.len(), 6, "5 financial + dangerous-eval");
        for fin in FINANCIAL_RULE_IDS {
            assert!(ids.contains(&fin.to_string()), "{fin} missing");
            assert!(
                !rule_limitations(fin).is_empty(),
                "{fin} must carry limitations"
            );
        }
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "rule ids must be sorted deterministically");
    }
}
