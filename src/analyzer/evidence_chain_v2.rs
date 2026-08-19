//! evidence_chain_v2 — composition chain attestation (G1 carried-patch
//! composition). ADDITIVE: evidence_chain_v1 is untouched.
//!
//! Same doctrine as v1: this verifier speaks ONLY about CHAIN
//! CONSISTENCY — it never declares the remediation correct. It attests
//! that a carried-patch composition's evidence is internally coherent:
//! the ordered member applications form a contiguous hash chain starting
//! from the anchored pristine baseline and ending at the composed state,
//! and a real, passing test report accompanies the composition.

use serde::{Deserialize, Serialize};

use super::operational::{now_stamp, OperationalStamp};

pub const COMPOSITION_CHAIN_SCHEMA_VERSION: &str = "evidence_chain_v2";

pub const OVERALL_EVIDENCED: &str = "composition_consistent_remediation_evidenced";
pub const OVERALL_INCONSISTENT: &str = "composition_inconsistent";
pub const OVERALL_INCOMPLETE: &str = "composition_incomplete";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionMemberEvidence {
    pub member_task_id: String,
    pub pre_image_blake3: String,
    pub post_image_blake3: String,
    pub applied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionInputs {
    pub target_file: String,
    pub baseline_hash: String,
    pub composed_state_blake3: String,
    pub members: Vec<CompositionMemberEvidence>,
    pub test_report_blake3: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionChainLink {
    pub name: String,
    /// verified | mismatch | missing
    pub status: String,
    pub expected: String,
    pub actual: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionChainCore {
    pub overall: String,
    pub links: Vec<CompositionChainLink>,
    pub inputs: CompositionInputs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceChainV2Report {
    pub schema_version: String,
    /// Content hash of `core`.
    pub chain_id: String,
    pub core: CompositionChainCore,
    /// Operational envelope: the only timestamp-bearing part.
    pub operational: OperationalStamp,
}

fn link(
    name: &str,
    status: &str,
    expected: &str,
    actual: &str,
    detail: &str,
) -> CompositionChainLink {
    CompositionChainLink {
        name: name.to_string(),
        status: status.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
        detail: detail.to_string(),
    }
}

/// Structural validation of a `test_report_v1` (same rules as v1's
/// validate_test_report): only a real passing run passes.
fn validate_test_report(bytes: &str) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(bytes).map_err(|e| format!("report is not valid JSON: {e}"))?;
    let get = |f: &str| value.get(f);
    let version = get("version")
        .and_then(|v| v.as_str())
        .ok_or("field 'version' missing")?;
    if version != "test_report_v1" {
        return Err(format!("unexpected report version '{version}'"));
    }
    let classification = get("classification")
        .and_then(|v| v.as_str())
        .ok_or("field 'classification' missing")?;
    if classification != "tests_passed" {
        return Err(format!(
            "classification is '{classification}', not tests_passed"
        ));
    }
    let passed = get("passed")
        .and_then(|v| v.as_bool())
        .ok_or("field 'passed' missing")?;
    if !passed {
        return Err("'passed' is false while classification claims tests_passed".to_string());
    }
    let exit_code = get("exit_code")
        .and_then(|v| v.as_i64())
        .ok_or("field 'exit_code' missing")?;
    if exit_code != 0 {
        return Err(format!("exit_code is {exit_code}, expected 0"));
    }
    let timed_out = get("timed_out")
        .and_then(|v| v.as_bool())
        .ok_or("field 'timed_out' missing")?;
    if timed_out {
        return Err("report is marked timed_out".to_string());
    }
    let argv = get("argv")
        .and_then(|v| v.as_array())
        .ok_or("field 'argv' missing")?;
    if argv.is_empty() {
        return Err("argv is empty — no command was evidenced".to_string());
    }
    Ok(())
}

/// Parse a composition evidence document (produced by the executor from
/// the combined apply-evidence artifact).
pub fn parse_composition_inputs(bytes: &str) -> Result<CompositionInputs, String> {
    let value: serde_json::Value = serde_json::from_str(bytes)
        .map_err(|e| format!("composition evidence is not valid JSON: {e}"))?;
    let target_file = value
        .get("composition_target_file")
        .and_then(|v| v.as_str())
        .ok_or("field 'composition_target_file' missing")?
        .to_string();
    let baseline_hash = value
        .get("composition_baseline_hash")
        .and_then(|v| v.as_str())
        .ok_or("field 'composition_baseline_hash' missing")?
        .to_string();
    let composed_state_blake3 = value
        .get("composed_state_blake3")
        .and_then(|v| v.as_str())
        .ok_or("field 'composed_state_blake3' missing")?
        .to_string();
    let members_arr = value
        .get("composition_members")
        .and_then(|v| v.as_array())
        .ok_or("field 'composition_members' missing")?;
    let mut members = Vec::new();
    for m in members_arr {
        members.push(CompositionMemberEvidence {
            member_task_id: m
                .get("member_task_id")
                .and_then(|v| v.as_str())
                .ok_or("member field 'member_task_id' missing")?
                .to_string(),
            pre_image_blake3: m
                .get("pre_image_blake3")
                .and_then(|v| v.as_str())
                .ok_or("member field 'pre_image_blake3' missing")?
                .to_string(),
            post_image_blake3: m
                .get("post_image_blake3")
                .and_then(|v| v.as_str())
                .ok_or("member field 'post_image_blake3' missing")?
                .to_string(),
            applied: m.get("applied").and_then(|v| v.as_bool()).unwrap_or(false),
        });
    }
    Ok(CompositionInputs {
        target_file,
        baseline_hash,
        composed_state_blake3,
        members,
        test_report_blake3: value
            .get("test_report_blake3")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

/// Verify the composition chain. Read-only; consistency-only.
pub fn verify_composition_chain(
    inputs: &CompositionInputs,
    test_report_bytes: Option<&str>,
) -> CompositionChainCore {
    let mut links = Vec::new();
    let mut inconsistent = false;
    let mut incomplete = false;

    // Link: members present.
    if inputs.members.is_empty() {
        links.push(link(
            "composition_members",
            "missing",
            ">=1 member",
            "0",
            "no members",
        ));
        incomplete = true;
    } else {
        links.push(link(
            "composition_members",
            "verified",
            ">=1 member",
            &inputs.members.len().to_string(),
            "",
        ));
    }

    // Link: all members applied.
    let all_applied = !inputs.members.is_empty() && inputs.members.iter().all(|m| m.applied);
    if inputs.members.is_empty() {
        // already marked missing
    } else if all_applied {
        links.push(link(
            "members_applied",
            "verified",
            "all applied=true",
            "all applied",
            "",
        ));
    } else {
        links.push(link(
            "members_applied",
            "mismatch",
            "all applied=true",
            "some applied=false",
            "",
        ));
        inconsistent = true;
    }

    // Link: baseline chain (first member starts from the anchored baseline).
    if let Some(first) = inputs.members.first() {
        if first.pre_image_blake3 == inputs.baseline_hash {
            links.push(link(
                "baseline_chain",
                "verified",
                &inputs.baseline_hash,
                &first.pre_image_blake3,
                "",
            ));
        } else {
            links.push(link(
                "baseline_chain",
                "mismatch",
                &inputs.baseline_hash,
                &first.pre_image_blake3,
                "first member does not start from the anchored baseline",
            ));
            inconsistent = true;
        }
    }

    // Link: member continuity (member[i].pre == member[i-1].post).
    let mut continuity_ok = true;
    for w in inputs.members.windows(2) {
        if w[0].post_image_blake3 != w[1].pre_image_blake3 {
            continuity_ok = false;
            break;
        }
    }
    if inputs.members.len() <= 1 {
        links.push(link(
            "member_continuity",
            "verified",
            "n/a",
            "single/none",
            "",
        ));
    } else if continuity_ok {
        links.push(link(
            "member_continuity",
            "verified",
            "contiguous",
            "contiguous",
            "",
        ));
    } else {
        links.push(link(
            "member_continuity",
            "mismatch",
            "contiguous",
            "broken",
            "member hash chain is not contiguous",
        ));
        inconsistent = true;
    }

    // Link: composed state equals the final member's post image.
    if let Some(last) = inputs.members.last() {
        if last.post_image_blake3 == inputs.composed_state_blake3 {
            links.push(link(
                "composed_state",
                "verified",
                &inputs.composed_state_blake3,
                &last.post_image_blake3,
                "",
            ));
        } else {
            links.push(link(
                "composed_state",
                "mismatch",
                &inputs.composed_state_blake3,
                &last.post_image_blake3,
                "composed state != final member post image",
            ));
            inconsistent = true;
        }
    }

    // Link: test report passed.
    match test_report_bytes {
        None => {
            links.push(link(
                "test_report_passed",
                "missing",
                "passing test_report_v1",
                "absent",
                "",
            ));
            incomplete = true;
        }
        Some(bytes) => match validate_test_report(bytes) {
            Ok(()) => links.push(link(
                "test_report_passed",
                "verified",
                "tests_passed",
                "tests_passed",
                "",
            )),
            Err(e) => {
                links.push(link(
                    "test_report_passed",
                    "mismatch",
                    "tests_passed",
                    "invalid",
                    &e,
                ));
                inconsistent = true;
            }
        },
    }

    let overall = if inconsistent {
        OVERALL_INCONSISTENT
    } else if incomplete {
        OVERALL_INCOMPLETE
    } else {
        OVERALL_EVIDENCED
    };

    CompositionChainCore {
        overall: overall.to_string(),
        links,
        inputs: inputs.clone(),
    }
}

/// Content hash of the core (deterministic, timestamp-free).
pub fn chain_id_for(core: &CompositionChainCore) -> String {
    let bytes = serde_json::to_vec(core).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
}

/// Write the composition chain report (JSON + MD). Returns the report.
pub fn write_composition_chain_report(
    core: &CompositionChainCore,
    out_dir: &std::path::Path,
) -> std::io::Result<EvidenceChainV2Report> {
    std::fs::create_dir_all(out_dir)?;
    let report = EvidenceChainV2Report {
        schema_version: COMPOSITION_CHAIN_SCHEMA_VERSION.to_string(),
        chain_id: chain_id_for(core),
        core: core.clone(),
        operational: now_stamp(),
    };
    let json_path = out_dir.join("evidence_chain_v2.json");
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&report).unwrap_or_default(),
    )?;
    let md = render_composition_md(&report);
    std::fs::write(out_dir.join("COMPOSITION_CHAIN_OF_CUSTODY.md"), md)?;
    Ok(report)
}

fn render_composition_md(report: &EvidenceChainV2Report) -> String {
    let mut out = String::new();
    out.push_str("# Composition Chain of Custody (evidence_chain_v2)\n\n");
    out.push_str(&format!("schema: {}\n", report.schema_version));
    out.push_str(&format!("chain_id: `{}`\n", report.chain_id));
    out.push_str(&format!("overall: **{}**\n\n", report.core.overall));
    out.push_str("This document attests CHAIN CONSISTENCY ONLY. It is NOT proof that the\nremediation is correct.\n\n");
    out.push_str("| link | status | expected | actual |\n|---|---|---|---|\n");
    for l in &report.core.links {
        out.push_str(&format!(
            "| {} | {} | `{}` | `{}` |\n",
            l.name, l.status, l.expected, l.actual
        ));
    }
    out.push_str(&format!("\ntarget: `{}`\n", report.core.inputs.target_file));
    out.push_str(&format!("members: {}\n", report.core.inputs.members.len()));
    out.push_str(&format!(
        "recorded_at: {}\n",
        report.operational.recorded_at_iso
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, pre: &str, post: &str) -> CompositionMemberEvidence {
        CompositionMemberEvidence {
            member_task_id: id.to_string(),
            pre_image_blake3: pre.to_string(),
            post_image_blake3: post.to_string(),
            applied: true,
        }
    }

    const PASSING_REPORT: &str = r#"{"version":"test_report_v1","classification":"tests_passed","passed":true,"exit_code":0,"timed_out":false,"argv":["python3","-","test.py"]}"#;

    #[test]
    fn contiguous_composition_with_passing_tests_is_evidenced() {
        let inputs = CompositionInputs {
            target_file: "ws/calc.py".to_string(),
            baseline_hash: "BASE".to_string(),
            composed_state_blake3: "M2POST".to_string(),
            members: vec![
                member("m1", "BASE", "M1POST"),
                member("m2", "M1POST", "M2POST"),
            ],
            test_report_blake3: Some("TR".to_string()),
        };
        let core = verify_composition_chain(&inputs, Some(PASSING_REPORT));
        assert_eq!(core.overall, OVERALL_EVIDENCED);
    }

    #[test]
    fn broken_continuity_is_inconsistent() {
        let inputs = CompositionInputs {
            target_file: "ws/calc.py".to_string(),
            baseline_hash: "BASE".to_string(),
            composed_state_blake3: "M2POST".to_string(),
            members: vec![
                member("m1", "BASE", "M1POST"),
                member("m2", "OTHER", "M2POST"),
            ],
            test_report_blake3: Some("TR".to_string()),
        };
        let core = verify_composition_chain(&inputs, Some(PASSING_REPORT));
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
    }

    #[test]
    fn first_member_not_at_baseline_is_inconsistent() {
        let inputs = CompositionInputs {
            target_file: "ws/calc.py".to_string(),
            baseline_hash: "BASE".to_string(),
            composed_state_blake3: "M1POST".to_string(),
            members: vec![member("m1", "NOTBASE", "M1POST")],
            test_report_blake3: Some("TR".to_string()),
        };
        let core = verify_composition_chain(&inputs, Some(PASSING_REPORT));
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
    }

    #[test]
    fn missing_members_is_incomplete() {
        let inputs = CompositionInputs {
            target_file: "ws/calc.py".to_string(),
            baseline_hash: "BASE".to_string(),
            composed_state_blake3: "X".to_string(),
            members: vec![],
            test_report_blake3: Some("TR".to_string()),
        };
        let core = verify_composition_chain(&inputs, Some(PASSING_REPORT));
        assert_eq!(core.overall, OVERALL_INCOMPLETE);
    }

    #[test]
    fn failing_test_report_is_inconsistent() {
        let inputs = CompositionInputs {
            target_file: "ws/calc.py".to_string(),
            baseline_hash: "BASE".to_string(),
            composed_state_blake3: "M1POST".to_string(),
            members: vec![member("m1", "BASE", "M1POST")],
            test_report_blake3: Some("TR".to_string()),
        };
        let failing = r#"{"version":"test_report_v1","classification":"tests_failed","passed":false,"exit_code":1,"timed_out":false,"argv":["python3"]}"#;
        let core = verify_composition_chain(&inputs, Some(failing));
        assert_eq!(core.overall, OVERALL_INCONSISTENT);
    }

    #[test]
    fn missing_test_report_is_incomplete() {
        let inputs = CompositionInputs {
            target_file: "ws/calc.py".to_string(),
            baseline_hash: "BASE".to_string(),
            composed_state_blake3: "M1POST".to_string(),
            members: vec![member("m1", "BASE", "M1POST")],
            test_report_blake3: None,
        };
        let core = verify_composition_chain(&inputs, None);
        assert_eq!(core.overall, OVERALL_INCOMPLETE);
    }

    #[test]
    fn chain_id_is_deterministic() {
        let inputs = CompositionInputs {
            target_file: "ws/calc.py".to_string(),
            baseline_hash: "BASE".to_string(),
            composed_state_blake3: "M1POST".to_string(),
            members: vec![member("m1", "BASE", "M1POST")],
            test_report_blake3: Some("TR".to_string()),
        };
        let a = verify_composition_chain(&inputs, Some(PASSING_REPORT));
        let b = verify_composition_chain(&inputs, Some(PASSING_REPORT));
        assert_eq!(chain_id_for(&a), chain_id_for(&b));
    }
}
