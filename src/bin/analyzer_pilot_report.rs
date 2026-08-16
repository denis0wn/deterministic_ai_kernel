//! analyzer_pilot_report — Enterprise Pilot Evidence Package generator.
//!
//! Usage:
//!   cargo run --bin analyzer_pilot_report -- \
//!     --workspace <path> --output <dir> [--repro FINDING_ID=PATH ...]
//!
//! Runs the read-only pipeline (ingestion → scan → triage → emitter with
//! readiness) and writes into --output:
//!   evidence_manifest_v1.json   (byte-stable, no timestamps)
//!   findings_v1.json            (byte-stable)
//!   task_contracts_v0.json      (byte-stable)
//!   PILOT_REPORT.md             (byte-stable)
//!   audit_log_v1.json           (OPERATIONAL: timestamps live only here)
//!
//! Invariants: the workspace is never modified; the output directory
//! must not lie inside the workspace; no executor is invoked — contracts
//! are proposals for a separate executor run. No LLM/MLX involvement:
//! this is a pure deterministic static pipeline.

use deterministic_ai_kernel::analyzer::pilot_report::{
    build_bundle, make_run_record, write_bundle_artifacts, OperationalRunLog,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn parse_repro(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--repro" {
            let spec = args
                .get(i + 1)
                .ok_or_else(|| "--repro requires FINDING_ID=PATH".to_string())?;
            let (id, path) = spec
                .split_once('=')
                .ok_or_else(|| format!("bad --repro spec (want FINDING_ID=PATH): {spec}"))?;
            if id.is_empty() || path.is_empty() {
                return Err(format!("bad --repro spec: {spec}"));
            }
            map.insert(id.to_string(), path.to_string());
            i += 2;
        } else {
            i += 1;
        }
    }
    Ok(map)
}

fn arg_value(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let workspace = match arg_value(&args, "--workspace") {
        Some(w) => w,
        None => {
            eprintln!(
                "usage: analyzer_pilot_report --workspace <path> --output <dir> [--repro FINDING_ID=PATH ...]"
            );
            std::process::exit(2);
        }
    };
    let out_dir = match arg_value(&args, "--output") {
        Some(o) => o,
        None => {
            eprintln!("--output <dir> is required");
            std::process::exit(2);
        }
    };
    let repro = match parse_repro(&args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let started = Instant::now();
    let ws_canonical = match workspace.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot resolve workspace {}: {e}", workspace.display());
            std::process::exit(1);
        }
    };

    // READ-ONLY invariant: analyzer artifacts must never land inside the
    // scanned workspace.
    let out_canonical_parent = out_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| out_dir.clone());
    let out_probe = if out_canonical_parent.exists() {
        match out_canonical_parent.canonicalize() {
            Ok(p) => p.join(out_dir.file_name().unwrap_or_default()),
            Err(_) => out_dir.clone(),
        }
    } else {
        out_dir.clone()
    };
    if out_probe.starts_with(&ws_canonical) {
        eprintln!(
            "refusing to write artifacts inside the scanned workspace ({} -> {})",
            out_dir.display(),
            ws_canonical.display()
        );
        std::process::exit(3);
    }

    // 1-4) read-only pipeline → deterministic artifact bundle.
    let bundle = match build_bundle(&ws_canonical, &repro) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("pilot bundle build failed: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "[pilot] workspace={} snapshot={} files={} findings={}",
        bundle.manifest.workspace,
        bundle.manifest.workspace_snapshot_blake3,
        bundle.manifest.inventory.file_count,
        bundle.findings.len()
    );
    for f in &bundle.findings {
        println!(
            "  {:?} {} — {} ({})",
            f.severity,
            f.id,
            f.candidate_statement,
            f.evidence.join(", ")
        );
    }

    let written = match write_bundle_artifacts(&bundle, &out_dir) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("artifact write failed: {e}");
            std::process::exit(1);
        }
    };

    // 5) OPERATIONAL audit log — the only artifact carrying time
    // metadata; excluded from all content hashes.
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let mut artifacts: BTreeMap<String, String> = BTreeMap::new();
    for (name, _path, hash) in &written {
        artifacts.insert(name.clone(), hash.clone());
    }
    let op_log = OperationalRunLog {
        started_at_iso: chrono::DateTime::from_timestamp(ts as i64, 0)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_else(|| ts.to_string()),
        ts_unix: ts,
        duration_ms: started.elapsed().as_millis() as u64,
        record: make_run_record(
            &bundle.manifest.run.run_id,
            ts,
            &bundle.manifest.workspace,
            &bundle,
            started.elapsed().as_millis() as u64,
        ),
        artifacts: artifacts.clone(),
    };
    let op_path = out_dir.join("audit_log_v1.json");
    let op_json = serde_json::to_string_pretty(&op_log).expect("operational log serialization");
    if let Err(e) = std::fs::write(&op_path, &op_json) {
        eprintln!("cannot write {}: {e}", op_path.display());
        std::process::exit(1);
    }
    let op_hash = blake3::hash(op_json.as_bytes()).to_hex().to_string();

    println!("[pilot] evidence package written to {}", out_dir.display());
    for (name, path, hash) in &written {
        println!("  {hash}  {path}  ({name})", path = path.display());
    }
    println!(
        "  {op_hash}  {path}  (audit_log_v1.json — operational, carries timestamps)",
        path = op_path.display()
    );
    println!("[pilot] workspace untouched; executor NOT invoked; no LLM used");
}
