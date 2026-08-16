//! analyzer_demo — end-to-end ANALYZER pipeline over a target workspace.
//!
//! Usage:
//!   cargo run --bin analyzer_demo -- <workspace> [--out <dir>]
//!
//! Sequence (ROLES.md layers): ingestion → scan_primitives → triage →
//! task_emitter → audit_log. The target workspace is READ-ONLY here; all
//! outputs go to the analyzer's own directories (default: analyzer_out/
//! and analyzer_logs/ in this repo). No effects are performed — the
//! emitted TaskContracts are consumed by the EXECUTOR separately.

use deterministic_ai_kernel::analyzer::{
    audit_log, ingestion, scan_primitives, task_emitter, triage, ANALYZER_VERSION,
};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn default_out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_out")
}

fn default_log_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_logs")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: analyzer_demo <workspace> [--out <dir>]");
        std::process::exit(2);
    }
    let workspace = PathBuf::from(&args[0]);
    let out_dir = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(default_out_dir);
    let log_dir = default_log_dir();
    let started = Instant::now();

    // 1) ingestion — read-only inventory.
    let inventory = match ingestion::scan_workspace(&workspace) {
        Ok(inv) => inv,
        Err(e) => {
            eprintln!("ingestion failed: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "[ingestion] workspace={} files={}",
        inventory.root,
        inventory.file_count()
    );

    // 2) scan primitives — static candidates (untrusted hints).
    let candidates = scan_primitives::run_static_scan(&inventory, &workspace);
    println!("[scan] candidates={}", candidates.len());

    // 3) triage — deterministic severity ranking (snapshot-anchored).
    let findings = triage::triage(&candidates, &inventory);
    println!("[triage] findings={}", findings.len());
    for f in &findings {
        println!(
            "  {:?} {} — {} ({})",
            f.severity,
            f.id,
            f.description,
            f.evidence.join(", ")
        );
    }

    // 4) task emitter — executor contracts (descriptions only, no effects).
    let ws_str = inventory.root.clone();
    let tasks = task_emitter::emit_tasks(&findings, &ws_str);
    println!("[emit] tasks={}", tasks.len());

    // 5) write outputs (analyzer's own dirs only).
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("cannot create out dir: {e}");
        std::process::exit(1);
    }
    let tasks_dir = out_dir.join("tasks");
    if let Err(e) = std::fs::create_dir_all(&tasks_dir) {
        eprintln!("cannot create tasks dir: {e}");
        std::process::exit(1);
    }
    let findings_json = serde_json::to_string_pretty(&findings).expect("serialize findings");
    let findings_path = out_dir.join("findings.json");
    std::fs::write(&findings_path, &findings_json).expect("write findings.json");
    for task in &tasks {
        let path = tasks_dir.join(format!("{}.json", task.finding.id));
        std::fs::write(
            &path,
            serde_json::to_string_pretty(task).expect("serialize task"),
        )
        .expect("write task json");
        println!("  -> {}", path.display());
    }

    // 6) audit trail.
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let record = audit_log::RunRecord {
        run_id: audit_log::make_run_id(&ws_str, ts),
        ts_unix: ts,
        workspace: ws_str.clone(),
        files_scanned: inventory.file_count(),
        candidates_found: candidates.len(),
        findings_count: findings.len(),
        tasks_emitted: tasks.iter().map(|t| t.finding.id.clone()).collect(),
        analyzer_version: ANALYZER_VERSION.to_string(),
        duration_ms: started.elapsed().as_millis() as u64,
    };
    match audit_log::write_run_record(&log_dir, &record) {
        Ok(p) => println!("[audit] {}", p.display()),
        Err(e) => {
            eprintln!("audit write failed: {e}");
            std::process::exit(1);
        }
    }
    println!(
        "[done] findings={} tasks={} (findings.json: {})",
        findings.len(),
        tasks.len(),
        findings_path.display()
    );
}
