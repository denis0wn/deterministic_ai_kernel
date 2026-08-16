//! analyzer_work_order — generate the passive remediation handoff
//! document for an APPROVED review decision (v0.4-pilot-ops).
//!
//! Usage:
//!   cargo run --bin analyzer_work_order -- \
//!     --package <pilot_out_dir> --decision <review_decision.json> \
//!     --out <dir>
//!
//! The work order executes NOTHING: it is a byte-stable document the
//! human operator carries to the executor side, with a checklist of the
//! evidence to bring back for analyzer_chain_verify.

use deterministic_ai_kernel::analyzer::pilot_package::load_evidence_package;
use deterministic_ai_kernel::analyzer::review_gate::load_review_decision;
use deterministic_ai_kernel::analyzer::work_order::write_work_order;
use std::path::PathBuf;

fn arg_value(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: analyzer_work_order --package <dir> --decision <decision.json> --out <dir>";
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
            eprintln!("work order refused: {e}");
            std::process::exit(1);
        }
    };
    let decision = match load_review_decision(&decision_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("work order refused: {e}");
            std::process::exit(1);
        }
    };

    match write_work_order(&pkg, &decision, &out) {
        Ok((json_path, md_path, order)) => {
            println!(
                "[work-order] finding={} work_order_id={}",
                order.core.finding_id, order.work_order_id
            );
            println!("[work-order] {}", json_path.display());
            println!("[work-order] {}", md_path.display());
            println!("[work-order] passive document: nothing executed, executor not invoked");
        }
        Err(e) => {
            eprintln!("work order refused (fail-closed): {e}");
            std::process::exit(1);
        }
    }
}
