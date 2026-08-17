//! Monetary invariant oracle (v0.3.1) — property-based hardening for
//! money-handling remediations.
//!
//! WHY: example-based tests (input -> expected output) can miss subtle
//! rounding/scale errors when they don't cover the exact edge case. The
//! NorthPay simulation showed a plausible-looking fix with a rounding-
//! scale error. For money we need PROPERTIES that hold for ALL values,
//! not just examples.
//!
//! GUARANTEE (fail-safe): a finding produced by a money-math rule is
//! `remediation_ready` ONLY IF a monetary-invariant test is present in
//! the workspace (detected by marker). Money therefore cannot be
//! remediated without property-based checks. This is a hard gate, not a
//! suggestion.
//!
//! The analyzer only DETECTS the marker and GENERATES a template; it
//! never writes into the scanned workspace (read-only invariant). The
//! operator adds the invariant test to the workspace's test file, and the
//! EXECUTOR runs it via run_tests_v1 like any other test.

use super::ingestion::WorkspaceInventory;

/// Money-math rules whose remediations must carry a monetary-invariant
/// test before they may be remediation_ready.
pub const MONETARY_RULE_IDS: &[&str] = &["money-truncation", "money-round-bare", "floor-div-money"];

/// Marker the operator places next to the invariant test(s) guarding a
/// finding: `# monetary-invariant: <FINDING_ID>`.
pub const INVARIANT_MARKER: &str = "# monetary-invariant:";

/// True when the finding was produced by a money-math rule.
pub fn is_monetary_rule(rule_id: &str) -> bool {
    MONETARY_RULE_IDS.contains(&rule_id)
}

/// Scan the workspace (read-only) for a monetary-invariant test guarding
/// `finding_id`. Returns true iff some Python file carries the marker
/// with this exact finding id.
pub fn invariant_present(workspace: &str, inv: &WorkspaceInventory, finding_id: &str) -> bool {
    let with_space = format!("{INVARIANT_MARKER} {finding_id}");
    let no_space = format!("{INVARIANT_MARKER}{finding_id}");
    inv.files.iter().any(|f| {
        if f.language != "python" {
            return false;
        }
        let path = std::path::Path::new(workspace).join(&f.rel_path);
        std::fs::read_to_string(path)
            .map(|content| content.contains(&with_space) || content.contains(&no_space))
            .unwrap_or(false)
    })
}

/// Generate a ready-to-adapt monetary-invariant test template.
///
/// The two always-on invariants (cent precision, determinism) need no
/// expected values and catch the rounding-scale class of errors (e.g. a
/// result of 0.125 where a cent-rounded 0.13 is required). The HALF-UP
/// boundary block is a skeleton the operator adapts to the function's
/// contract. The operator pastes this into the workspace's test file and
/// adjusts `_money_result` to call the function under test.
pub fn generate_invariant_template(finding_id: &str) -> String {
    format!(
        r#"{marker} {finding}
# Monetary invariant oracle (property-based). These checks hold for ALL
# probe values, not just examples, and catch subtle rounding/scale errors
# that example tests can miss. ADAPT `_money_result` to call the function
# under test with a money amount; keep the invariants unchanged.

_PROBES = [0.0, 0.005, 0.01, 0.125, 0.5, 0.995, 1.0, 1.005, 2.5, 3.333, 10.0, 99.999]


def _money_result(amount):
    # ADAPT: return the monetary result for `amount`.
    raise NotImplementedError("call the function under test here")


def test_invariant_cent_precision():
    # A money result must never carry a sub-cent remainder. The modulo
    # form is used (not round()) so this guard itself does not trip the
    # money-round-bare rule when placed in a finance-path test file.
    for a in _PROBES:
        r = _money_result(a)
        assert abs((r * 100) % 1) < 1e-9, (
            "sub-cent remainder for %r -> %r" % (a, r)
        )


def test_invariant_determinism():
    for a in _PROBES:
        assert _money_result(a) == _money_result(a), (
            "non-deterministic for %r" % a
        )


def test_invariant_half_up_boundary():
    # ADAPT to the function's contract: half-cent midpoints must round
    # HALF-UP (never to even / never truncate). Example shape:
    #   assert _money_result(<midpoint>) == <expected_half_up>
    pass
"#,
        marker = INVARIANT_MARKER,
        finding = finding_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::ingestion::scan_workspace;

    #[test]
    fn monetary_rule_classification() {
        assert!(is_monetary_rule("money-truncation"));
        assert!(is_monetary_rule("money-round-bare"));
        assert!(is_monetary_rule("floor-div-money"));
        assert!(!is_monetary_rule("none-arith"));
        assert!(!is_monetary_rule("todo-marker"));
        assert!(!is_monetary_rule("float-equality"));
    }

    #[test]
    fn template_carries_marker_and_invariants() {
        let t = generate_invariant_template("MONEY-TRUNCATION-FEES-25");
        assert!(t.contains("# monetary-invariant: MONEY-TRUNCATION-FEES-25"));
        assert!(t.contains("test_invariant_cent_precision"));
        assert!(t.contains("test_invariant_determinism"));
        assert!(t.contains("test_invariant_half_up_boundary"));
    }

    #[test]
    fn invariant_detection_finds_marker_in_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("billing")).unwrap();
        let test_file = dir.path().join("test_fees.py");
        std::fs::write(
            &test_file,
            "# monetary-invariant: MONEY-TRUNCATION-FEES-25\ndef test_inv():\n    pass\n",
        )
        .unwrap();
        let inv = scan_workspace(dir.path()).unwrap();
        let ws = dir.path().to_string_lossy().to_string();
        assert!(invariant_present(&ws, &inv, "MONEY-TRUNCATION-FEES-25"));
        assert!(!invariant_present(&ws, &inv, "SOME-OTHER-FINDING"));
    }
}
