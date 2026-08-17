//! analyzer_monetary_oracle — print a ready-to-adapt monetary-invariant
//! test template for a finding.
//!
//! Usage:
//!   cargo run --bin analyzer_monetary_oracle -- --finding <FINDING_ID>
//!
//! The analyzer is read-only: this tool only PRINTS the template. The
//! operator pastes it into the workspace's test file and adapts
//! `_money_result`. Once present (marker detected), a money-math finding
//! becomes remediation_ready; without it, money can never be remediated.

use deterministic_ai_kernel::analyzer::monetary_oracle;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: analyzer_monetary_oracle --finding <FINDING_ID>";
    let finding = match args.iter().position(|a| a == "--finding") {
        Some(i) => args.get(i + 1).cloned(),
        None => None,
    };
    let finding = match finding {
        Some(f) if !f.is_empty() => f,
        _ => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    print!("{}", monetary_oracle::generate_invariant_template(&finding));
}
