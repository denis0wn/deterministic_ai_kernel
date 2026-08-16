//! analyzer_review — record a human review decision for a finding in an
//! evidence package (v0.4-pilot-ops).
//!
//! Usage:
//!   cargo run --bin analyzer_review -- \
//!     --package <pilot_out_dir> --finding <ID> \
//!     --decision approve|reject|defer \
//!     --reviewer <text> --rationale <text> \
//!     [--repro <workspace-relative-path>] [--override-candidate-only] \
//!     --out <dir>
//!
//! This tool records the HUMAN boundary: it validates package integrity
//! (fail-closed) and writes a tamper-evident review_decision_v1.json.
//! It performs no remediation and never invokes the executor.

use deterministic_ai_kernel::analyzer::pilot_package::load_evidence_package;
use deterministic_ai_kernel::analyzer::review_gate::{
    record_review, ReviewInput, DECISION_APPROVE,
};
use std::path::PathBuf;

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: analyzer_review --package <dir> --finding <ID> --decision approve|reject|defer --reviewer <text> --rationale <text> [--repro <path>] [--override-candidate-only] --out <dir>";
    let package = match arg_value(&args, "--package") {
        Some(v) => PathBuf::from(v),
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let finding = match arg_value(&args, "--finding") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let decision = match arg_value(&args, "--decision") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let reviewer = arg_value(&args, "--reviewer").unwrap_or_default();
    let rationale = arg_value(&args, "--rationale").unwrap_or_default();
    let repro = arg_value(&args, "--repro");
    let override_flag = args.iter().any(|a| a == "--override-candidate-only");
    let out = match arg_value(&args, "--out") {
        Some(v) => PathBuf::from(v),
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };

    let pkg = match load_evidence_package(&package) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("review refused: {e}");
            std::process::exit(1);
        }
    };

    let input = ReviewInput {
        finding_id: finding,
        decision,
        reviewer,
        rationale,
        repro_test_path: repro,
        override_candidate_only: override_flag,
    };

    match record_review(&pkg, &input, &out) {
        Ok((path, d)) => {
            let ov = if d.decision_core.override_candidate_only {
                " (override_candidate_only recorded)"
            } else {
                ""
            };
            println!(
                "[review] decision={} finding={} id={}{}",
                d.decision_core.decision, d.decision_core.finding_id, d.decision_id, ov
            );
            if d.decision_core.decision == DECISION_APPROVE {
                println!(
                    "[review] next: analyzer_work_order --package {} --decision {} --out <dir>",
                    package.display(),
                    path.display()
                );
            }
            println!("[review] written: {}", path.display());
            println!("[review] no remediation performed; executor not invoked");
        }
        Err(e) => {
            eprintln!("review refused (fail-closed): {e}");
            std::process::exit(1);
        }
    }
}
