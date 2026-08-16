//! analyzer_chain_verify — independent verification of the remediation
//! evidence chain (v0.4-pilot-ops).
//!
//! Usage:
//!   cargo run --bin analyzer_chain_verify -- \
//!     --package <pilot_out_dir> --decision <review_decision.json> \
//!     --pre-dir <isolated copy before executor> \
//!     --post-dir <isolated copy after executor> \
//!     --test-report <test_report_v1.json> \
//!     [--event-log <event_log.json>] --out <dir>
//!
//! Read-only over every input. All hashes are recomputed from bytes;
//! executor self-reported statuses are structurally validated, never
//! believed. The output speaks only about CHAIN CONSISTENCY — never
//! about the correctness of the fix.

use deterministic_ai_kernel::analyzer::evidence_chain::write_chain_report;
use deterministic_ai_kernel::analyzer::pilot_package::load_evidence_package;
use deterministic_ai_kernel::analyzer::review_gate::load_review_decision;
use std::path::PathBuf;

fn arg_value(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: analyzer_chain_verify --package <dir> --decision <decision.json> --pre-dir <dir> --post-dir <dir> --test-report <file> [--event-log <file>] --out <dir>";
    let package = match arg_value(&args, "--package") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let decision_path = match arg_value(&args, "--decision") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let pre_dir = match arg_value(&args, "--pre-dir") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let post_dir = match arg_value(&args, "--post-dir") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let test_report = match arg_value(&args, "--test-report") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    let event_log = arg_value(&args, "--event-log");
    let out = match arg_value(&args, "--out") {
        Some(v) => v,
        None => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };

    let pkg = match load_evidence_package(&package) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("chain verify refused: {e}");
            std::process::exit(1);
        }
    };
    let decision = match load_review_decision(&decision_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("chain verify refused: {e}");
            std::process::exit(1);
        }
    };

    match write_chain_report(
        &pkg,
        &decision,
        &pre_dir,
        &post_dir,
        &test_report,
        event_log.as_deref(),
        &out,
    ) {
        Ok((json_path, md_path, report)) => {
            println!(
                "[chain] overall={} chain_id={}",
                report.core.overall, report.chain_id
            );
            for l in &report.core.links {
                println!("  [link] {}={}: {}", l.name, l.status, l.detail);
            }
            println!("[chain] {}", json_path.display());
            println!("[chain] {}", md_path.display());
            println!("[chain] chain consistency is not proof of fix correctness; inputs untouched");
        }
        Err(e) => {
            eprintln!("chain verify failed: {e}");
            std::process::exit(1);
        }
    }
}
