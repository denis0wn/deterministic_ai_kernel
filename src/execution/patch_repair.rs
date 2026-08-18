//! S3 escape repair (PROGRESS UNTIL VERIFIED stage 3).
//!
//! Deterministic, kernel-owned repair of ONE corruption class in
//! model-produced patch_v1 JSON: at a serde "invalid escape" site, a
//! backslash followed by one or more spaces/tabs is repaired to the
//! `\n` escape, preserving the following whitespace (observed NorthPay
//! corruption: hard-wrap points copied as `\` + indent instead of
//! `\n` + indent).
//!
//! Design-review conditions honored (DESIGN_REVIEW_ESCAPE_REPAIR.md):
//! - D1: scope frozen at pattern R1; extension only on new evidence.
//! - D2: grounded/ungrounded asymmetry — the whole repair is ACCEPTED
//!   only if the repaired `context_before` matches the target file
//!   byte-for-byte exactly once (the existing grounding check is the
//!   acceptance criterion). Replacement repair inherits the anchor:
//!   no grounded context ⇒ the entire repair is rejected.
//! - D4: pure and deterministic (same input ⇒ same output).
//! - D5: the caller records a RepairReport (sites + raw/repaired
//!   BLAKE3) into the step's extra_output — no invisible repairs.
//! - D7: ≤ 8 sites, exactly one candidate per site (the R1 transform
//!   is single-valued), repair-only parse errors, single pass.
//! - D8: kill switch `DAK_PATCH_ESCAPE_REPAIR=off`.
//!
//! target_file / version / reason are OUT OF SCOPE: corruption there
//! stays on the existing terminal path.

use crate::execution::patch_contract::{self, PatchV1};

/// Maximum number of repaired sites per output (D7).
pub const MAX_REPAIR_SITES: usize = 8;

/// One repaired corruption site.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairSite {
    /// Byte offset of the corrupted backslash in the text at repair
    /// time (raw output bytes are recorded alongside, so the site is
    /// fully auditable).
    pub byte_offset: usize,
    /// Corrupted fragment (backslash + up to 4 following chars).
    pub before: String,
    /// Repaired fragment.
    pub after: String,
    /// Pattern id (frozen at R1).
    pub pattern: String,
}

/// Audit record of an applied repair (D5).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RepairReport {
    pub sites: Vec<RepairSite>,
    pub raw_blake3: String,
    pub repaired_blake3: String,
}

/// Kill switch (D8): repair is enabled unless the env var is set to
/// off/0/false.
pub fn repair_enabled() -> bool {
    match std::env::var("DAK_PATCH_ESCAPE_REPAIR") {
        Ok(v) => !matches!(v.to_ascii_lowercase().as_str(), "off" | "0" | "false"),
        Err(_) => true,
    }
}

/// Convert a serde (line, column) position to a byte offset, walking
/// lines defensively (model output is normally single-line).
fn position_to_byte_offset(text: &str, line: usize, col: usize) -> usize {
    let mut offset = 0usize;
    for (i, l) in text.split('\n').enumerate() {
        if i + 1 == line {
            return offset + col.saturating_sub(1).min(l.len());
        }
        offset += l.len() + 1;
    }
    text.len()
}

/// Parse the (line, column) pair out of a serde_json error message
/// ("... at line L column C"). Message-format parsing keeps this
/// independent of serde_json API versions.
fn parse_position(msg: &str) -> Option<(usize, usize)> {
    let line = msg
        .rsplit("line ")
        .next()?
        .split_whitespace()
        .next()?
        .parse::<usize>()
        .ok()?;
    let col = msg
        .rsplit("column ")
        .next()?
        .split_whitespace()
        .next()?
        .parse::<usize>()
        .ok()?;
    Some((line, col))
}

/// Attempt the bounded R1 repair of extracted patch JSON, then validate
/// the repaired patch against the patch_v1 schema AND the grounding
/// anchor (context_before matches file_content exactly once). Any
/// ambiguity or non-coverage is a hard Err — the caller falls through
/// to the existing model-repair retry / terminal path.
pub fn repair_patch_json(
    json_text: &str,
    file_content: &str,
) -> Result<(PatchV1, RepairReport), String> {
    let raw_blake3 = blake3::hash(json_text.as_bytes()).to_hex().to_string();
    let mut work = json_text.to_string();
    let mut sites: Vec<RepairSite> = Vec::new();

    let value: serde_json::Value = loop {
        match serde_json::from_str::<serde_json::Value>(&work) {
            Ok(v) => break v,
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("invalid escape") {
                    return Err(format!("not an invalid-escape parse problem: {msg}"));
                }
                if sites.len() >= MAX_REPAIR_SITES {
                    return Err(format!(
                        "repair bound exceeded (>{} sites)",
                        MAX_REPAIR_SITES
                    ));
                }
                let (line, col) = parse_position(&msg)
                    .ok_or_else(|| format!("unparsable escape-error position: {msg}"))?;
                let idx = position_to_byte_offset(&work, line, col);
                // The serde invalid-escape position points at the escaped
                // character; the offending backslash is immediately before
                // it (checked at idx-1, then idx defensively).
                let bs = [idx.saturating_sub(1), idx.min(work.len().saturating_sub(1))]
                    .into_iter()
                    .find(|&cand| {
                        cand < work.len()
                            && work.as_bytes()[cand] == b'\\'
                            && work[cand + 1..]
                                .chars()
                                .next()
                                .map(|c| c == ' ' || c == '\t')
                                .unwrap_or(false)
                    })
                    .ok_or_else(|| format!("unsupported escape near byte {idx} (no R1 pattern)"))?;
                // R1 (single-valued): backslash + [ \t]+  →  \n + same
                // whitespace. The whitespace itself is preserved.
                let ws_len = work[bs + 1..]
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .map(|c| c.len_utf8())
                    .sum::<usize>();
                let tail = work[bs + 1..(bs + 1 + ws_len).min(bs + 5)].to_string();
                let before = format!("\\{tail}");
                let after = format!("\\n{tail}");
                work.replace_range(bs..bs + 1, "\\n");
                sites.push(RepairSite {
                    byte_offset: bs,
                    before,
                    after,
                    pattern: "R1".to_string(),
                });
            }
        }
    };

    // Strict patch_v1 schema on the repaired object.
    let patch: PatchV1 = serde_json::from_value(value)
        .map_err(|e| format!("repaired JSON is not a valid patch_v1: {e}"))?;

    // Shape gate applies to repaired patches exactly as to model-clean
    // ones (D6).
    patch_contract::validate_patch_shape(&patch)
        .map_err(|e| format!("repaired patch failed shape gate: {e}"))?;

    // D2 anchor: repaired context_before must be grounded byte-for-byte
    // exactly once. This is also what makes replacement repair safe: it
    // is accepted only behind a uniquely grounded context.
    let occurrences = patch_contract::validate_patch_against_content(&patch, file_content)
        .map_err(|e| format!("repair rejected — context not grounded: {e}"))?;
    if occurrences != 1 {
        return Err(format!(
            "repair rejected — context matched {occurrences} times, need exactly 1"
        ));
    }

    let repaired_blake3 = blake3::hash(work.as_bytes()).to_hex().to_string();
    Ok((
        patch,
        RepairReport {
            sites,
            raw_blake3,
            repaired_blake3,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "def total(xs):\n    \"\"\"Doc line one wraps\n    here on line two.\"\"\"\n    return sum(xs)\n";

    fn good_patch_json(context: &str, replacement: &str) -> String {
        serde_json::json!({
            "version": "patch_v1",
            "target_file": "t.py",
            "context_before": context,
            "replacement": replacement,
            "reason": "test"
        })
        .to_string()
    }

    /// Corrupt a newline-escape the way the model does: `\n` + indent
    /// becomes `\` + indent (the 'n' is dropped).
    fn corrupt(json: &str) -> String {
        json.replace("\\n    ", "\\    ")
    }

    #[test]
    fn r1_repairs_single_corrupted_wrap() {
        let context = "Doc line one wraps\n    here on line two.";
        let clean = good_patch_json(context, "REPLACEMENT");
        let corrupted = corrupt(&clean);
        assert!(serde_json::from_str::<serde_json::Value>(&corrupted).is_err());
        let (patch, report) = repair_patch_json(&corrupted, FILE).expect("test failure");
        assert_eq!(patch.context_before, context);
        assert_eq!(patch.replacement, "REPLACEMENT");
        assert_eq!(report.sites.len(), 1);
        assert_eq!(report.sites[0].pattern, "R1");
        assert_ne!(report.raw_blake3, report.repaired_blake3);
    }

    #[test]
    fn r1_repairs_multiple_sites_in_context_and_replacement() {
        let context = "Doc line one wraps\n    here on line two.";
        let replacement = "new code wraps\n    here too.";
        let clean = good_patch_json(context, replacement);
        let corrupted = corrupt(&clean);
        let (patch, report) = repair_patch_json(&corrupted, FILE).expect("test failure");
        assert_eq!(patch.context_before, context);
        assert_eq!(patch.replacement, replacement);
        assert_eq!(report.sites.len(), 2);
    }

    #[test]
    fn ungrounded_context_rejects_whole_repair() {
        // Replacement would be repairable, but the context does not
        // exist in the file ⇒ D2 anchor fails ⇒ full rejection.
        let context = "this text is NOT in the file\n    at all.";
        let clean = good_patch_json(context, "x wraps\n    y.");
        let corrupted = corrupt(&clean);
        let err = repair_patch_json(&corrupted, FILE).unwrap_err();
        assert!(err.contains("not grounded"), "got: {err}");
    }

    #[test]
    fn ambiguous_context_rejects_whole_repair() {
        // "return sum(xs)" appears exactly once; build a file where the
        // context occurs twice.
        let file = "a wraps\n    b.\na wraps\n    b.\n";
        let context = "a wraps\n    b.";
        let clean = good_patch_json(context, "c");
        let corrupted = corrupt(&clean);
        let err = repair_patch_json(&corrupted, file).unwrap_err();
        assert!(
            err.contains("exactly 1") || err.contains("not grounded"),
            "got: {err}"
        );
    }

    #[test]
    fn non_escape_errors_are_not_repaired() {
        let broken = r#"{"version":"patch_v1","#; // truncated JSON, no escape issue
        let err = repair_patch_json(broken, FILE).unwrap_err();
        assert!(err.contains("not an invalid-escape"), "got: {err}");
    }

    #[test]
    fn unsupported_escape_is_not_repaired() {
        // `\q` is an invalid escape but not the R1 pattern.
        let broken = r#"{"version":"patch_v1","target_file":"t.py","context_before":"bad \q escape","replacement":"r","reason":"x"}"#;
        let err = repair_patch_json(broken, FILE).unwrap_err();
        assert!(err.contains("unsupported escape"), "got: {err}");
    }

    #[test]
    fn bound_exceeding_is_rejected() {
        // 9 corrupted wraps > MAX_REPAIR_SITES.
        let mut context = String::from("start");
        for _ in 0..9 {
            context.push_str("\n    cont");
        }
        let file = format!("{context}\n");
        let clean = good_patch_json(&context, "r");
        let corrupted = corrupt(&clean);
        let err = repair_patch_json(&corrupted, &file).unwrap_err();
        assert!(err.contains("bound exceeded"), "got: {err}");
    }

    #[test]
    fn valid_json_is_left_alone_by_callers_contract() {
        // repair_patch_json is only invoked after a parse failure, but
        // it must still behave sanely on valid input: it parses, yet the
        // context is not in FILE ⇒ rejection (never silent acceptance).
        let clean = good_patch_json("not in file", "r");
        assert!(repair_patch_json(&clean, FILE).is_err());
    }

    #[test]
    fn repair_is_deterministic() {
        let context = "Doc line one wraps\n    here on line two.";
        let corrupted = corrupt(&good_patch_json(context, "R"));
        let a = repair_patch_json(&corrupted, FILE).expect("test failure");
        let b = repair_patch_json(&corrupted, FILE).expect("test failure");
        assert_eq!(a.1.repaired_blake3, b.1.repaired_blake3);
        assert_eq!(a.0.replacement, b.0.replacement);
    }

    #[test]
    fn kill_switch_env_disables_repair() {
        // repair_enabled is a pure env read; verify the off values.
        std::env::set_var("DAK_PATCH_ESCAPE_REPAIR", "off");
        assert!(!repair_enabled());
        std::env::set_var("DAK_PATCH_ESCAPE_REPAIR", "0");
        assert!(!repair_enabled());
        std::env::set_var("DAK_PATCH_ESCAPE_REPAIR", "on");
        assert!(repair_enabled());
        std::env::remove_var("DAK_PATCH_ESCAPE_REPAIR");
        assert!(repair_enabled());
    }

    #[test]
    fn noop_repair_rejected_by_shape_gate() {
        // context == replacement after repair ⇒ shape gate refuses.
        let context = "Doc line one wraps\n    here on line two.";
        let clean = good_patch_json(context, context);
        let corrupted = corrupt(&clean);
        let err = repair_patch_json(&corrupted, FILE).unwrap_err();
        assert!(err.contains("shape gate"), "got: {err}");
    }
}
