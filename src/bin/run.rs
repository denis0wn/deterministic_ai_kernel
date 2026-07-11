//! deterministic_ai_kernel CLI
//!
//! Subcommands:
//!   run      --payload "..." --seed 42 [--store ./runs]
//!   verify   --store ./runs
//!   diff     --store ./runs PLAN_ID_A PLAN_ID_B

use anyhow::{bail, Result};
use std::env;

use deterministic_ai_kernel::planner_pipeline::execution_engine::{ExecutionEngine, StepStatus};
use deterministic_ai_kernel::planner_pipeline::persistence::{PersistenceStore, StoreConfig};
use deterministic_ai_kernel::planner_pipeline::pipeline::Pipeline;
use deterministic_ai_kernel::planner_pipeline::plan_diff::{PlanDiff, StepChange};
use deterministic_ai_kernel::planner_pipeline::replay::ReplayTape;
use deterministic_ai_kernel::planner_pipeline::PipelineContext;
use deterministic_ai_kernel::planner_pipeline::Plan;
use deterministic_ai_kernel::semantic_bias::{BiasConfiguration, BiasVersion, SemanticBiasRule};

fn default_bias() -> BiasConfiguration {
    BiasConfiguration::new(
        "default-v1",
        vec![SemanticBiasRule::new(
            "critical-first",
            1,
            "critical",
            "first",
        )],
    )
}

fn make_store(dir: &str) -> Result<PersistenceStore> {
    PersistenceStore::new(StoreConfig::new(dir))
}

fn print_usage(code: i32) -> ! {
    println!("Usage:");
    println!("  run run      --payload <text> --seed <u64> [--store <dir>]");
    println!("  run verify   --store <dir>");
    println!("  run diff                  --store <dir> <PLAN_ID_A> <PLAN_ID_B>");
    println!("  run emit-bias-artifact    <task_bias_id> <step_bias_id> [preferred...]");
    println!("  run latest-bias-artifact  <task_bias_id> <step_bias_id>");
    println!("  run analyze-task          --payload <text> --seed <u64>");
    std::process::exit(code);
}

// -- subcommand: run ----------------------------------------------------------

fn cmd_run(args: &[String]) -> Result<()> {
    let mut payload = None;
    let mut seed: Option<u64> = None;
    let mut store_dir = "./runs".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--payload" => {
                i += 1;
                payload = Some(args[i].clone());
            }
            "--seed" => {
                i += 1;
                seed = Some(args[i].parse()?);
            }
            "--store" => {
                i += 1;
                store_dir = args[i].clone();
            }
            other => bail!("unknown argument: {other}"),
        }
        i += 1;
    }
    let payload = payload.ok_or_else(|| anyhow::anyhow!("--payload required"))?;
    let seed = seed.ok_or_else(|| anyhow::anyhow!("--seed required"))?;

    let ctx = PipelineContext {
        seed,
        bias_version: BiasVersion::V1,
    };
    let engine = ExecutionEngine::with_default_executor(Pipeline::new(default_bias()));
    let store = make_store(&store_dir)?;
    let mut tape = store.load_tape().unwrap_or_else(|_| ReplayTape::new());

    let report = engine.run_with_replay(&payload, &ctx, &mut tape)?;
    store.save_report(&report)?;
    store.save_tape(&tape)?;

    println!("plan_id : {}", report.plan_id);
    println!("seed    : {}", report.seed);
    println!("steps   : {}", report.steps.len());
    println!("success : {}", report.success);
    println!("time_ms : {}", report.total_duration_ms);
    println!();
    for s in &report.steps {
        let status = match &s.status {
            StepStatus::Ok => "ok".to_string(),
            StepStatus::Skipped => "skip".to_string(),
            StepStatus::Failed(e) => format!("FAIL: {e}"),
        };
        println!("  [{:>2}] {} -- {}", s.index, status, s.description);
    }
    println!();
    println!("tape    : {} entries  (store: {store_dir})", tape.len());
    if !report.success {
        std::process::exit(1);
    }
    Ok(())
}

// -- subcommand: verify -------------------------------------------------------

fn cmd_verify(args: &[String]) -> Result<()> {
    let mut store_dir = "./runs".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--store" => {
                i += 1;
                store_dir = args[i].clone();
            }
            other => bail!("unknown argument: {other}"),
        }
        i += 1;
    }

    let store = make_store(&store_dir)?;
    let tape = store.load_tape()?;
    let entries = tape.entries().to_vec();

    println!("Replay Verification");
    println!("store : {store_dir}");
    println!("tape  : {} entries", entries.len());
    println!();

    if entries.is_empty() {
        println!("Replay status : PASSED");
        println!("Drift         : 0");
        return Ok(());
    }

    let pipeline = Pipeline::new(default_bias());
    let mut drift = 0usize;
    let mut failed: Vec<(usize, String, u64, String)> = vec![];

    for (idx, entry) in entries.iter().enumerate() {
        let ctx = PipelineContext {
            seed: entry.seed,
            bias_version: BiasVersion::V1,
        };
        let out = pipeline.run(entry.payload.clone(), &ctx)?;
        let actual = out.plan.id.clone();

        if actual == entry.plan_id {
            println!("  [ok] tape #{}", idx + 1);
        } else {
            println!("  [!!] tape #{}", idx + 1);
            drift += 1;
            failed.push((
                idx + 1,
                entry.payload.clone(),
                entry.seed,
                entry.plan_id.clone(),
            ));
        }
    }

    println!();

    if drift == 0 {
        println!("Replay status : PASSED");
        println!("Drift         : 0");
        return Ok(());
    }

    println!("Replay status : FAILED");
    println!("Drift         : {drift}");
    println!();

    for (tape_no, payload, seed, expected_id) in failed {
        let ctx = PipelineContext {
            seed,
            bias_version: BiasVersion::V1,
        };
        let actual = pipeline.run(payload.clone(), &ctx)?;

        println!("Tape #{tape_no}");
        println!("Expected:");
        println!("  plan_id = {expected_id}");
        println!("Actual:");
        println!("  plan_id = {}", actual.plan.id);

        match store.load_report(&expected_id) {
            Ok(saved) => {
                let old_plan = Plan::new_with_stable_id(
                    seed,
                    saved.steps.iter().map(|s| s.description.clone()).collect(),
                );
                let diff = PlanDiff::diff(&old_plan, &actual.plan);
                println!("Drift:");
                for ch in diff.changes {
                    match ch {
                        StepChange::Added(s) => println!("    + {s}"),
                        StepChange::Removed(s) => println!("    - {s}"),
                        StepChange::Retained(_) => {}
                    }
                }
            }
            Err(_) => println!(
                "Drift:
  (saved run not found)"
            ),
        }
        println!();
    }

    std::process::exit(1);
}

// -- subcommand: diff ---------------------------------------------------------

fn cmd_diff(args: &[String]) -> Result<()> {
    let mut store_dir = "./runs".to_string();
    let mut positional: Vec<String> = vec![];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--store" => {
                i += 1;
                store_dir = args[i].clone();
            }
            other => positional.push(other.to_owned()),
        }
        i += 1;
    }

    if positional.len() != 2 {
        bail!("diff requires exactly two plan IDs: diff --store <dir> <PLAN_A> <PLAN_B>");
    }

    let id_a = &positional[0];
    let id_b = &positional[1];

    let store = make_store(&store_dir)?;
    let report_a = store
        .load_report(id_a)
        .map_err(|_| anyhow::anyhow!("plan {id_a} not found in {store_dir}"))?;
    let report_b = store
        .load_report(id_b)
        .map_err(|_| anyhow::anyhow!("plan {id_b} not found in {store_dir}"))?;

    let plan_a = Plan::new_with_stable_id(
        report_a.seed,
        report_a
            .steps
            .iter()
            .map(|s| s.description.clone())
            .collect(),
    );
    let plan_b = Plan::new_with_stable_id(
        report_b.seed,
        report_b
            .steps
            .iter()
            .map(|s| s.description.clone())
            .collect(),
    );

    let diff = PlanDiff::diff(&plan_a, &plan_b);

    println!("PlanDiff");
    println!("  A : {id_a}");
    println!("  B : {id_b}");
    println!();

    for ch in &diff.changes {
        match ch {
            StepChange::Added(s) => println!("  + {s}"),
            StepChange::Removed(s) => println!("  - {s}"),
            StepChange::Retained(s) => println!("  ~ {s}"),
        }
    }

    println!();
    if diff.is_identical() {
        println!("Plans are identical.");
    } else {
        println!("Added   : {}", diff.added().len());
        println!("Removed : {}", diff.removed().len());
    }

    Ok(())
}

// -- main ---------------------------------------------------------------------

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let rest = if args.len() > 2 { &args[2..] } else { &[] };

    match subcmd {
        "run" => cmd_run(rest),
        "verify" => cmd_verify(rest),
        "diff" => cmd_diff(rest),
        "emit-bias-artifact" => cmd_emit_bias_artifact(rest),
        "latest-bias-artifact" => cmd_latest_bias_artifact(rest),
        "analyze-task" => cmd_analyze_task(rest),
        "doctor-json" => cmd_doctor_json(rest),
        "--help" | "help" | "-h" => print_usage(0),
        _ => print_usage(2),
    }
}

// -- subcommand: emit-bias-artifact ------------------------------------------

fn cmd_emit_bias_artifact(args: &[String]) -> Result<()> {
    use rusqlite::{params, Connection};
    use std::collections::HashMap;

    if args.len() < 2 {
        bail!("emit-bias-artifact <task_bias_id> <step_bias_id> [preferred_step...]");
    }
    let task_bias_id = &args[0];
    let step_bias_id = &args[1];
    let preferred: Vec<&str> = args[2..].iter().map(|s| s.as_str()).collect();

    let db_path = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| "kernel.db".to_string());

    let conn = Connection::open(&db_path)?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS bias_artifacts (
            id            TEXT PRIMARY KEY,
            task_bias_id  TEXT NOT NULL,
            step_bias_id  TEXT NOT NULL,
            created_at    INTEGER NOT NULL,
            artifact_type TEXT NOT NULL,
            payload       TEXT NOT NULL
        );
    ",
    )?;

    // build payload
    let weights: HashMap<&str, f64> = preferred.iter().map(|k| (*k, 1.0_f64)).collect();
    let mut lines = vec![
        "bias.version=v1".to_string(),
        format!("bias.meta.weighted_count={}", preferred.len()),
    ];
    for p in &preferred {
        lines.push(format!("bias.weight.{p}=1.000000"));
    }

    let payload = serde_json::json!({
        "version": "v1",
        "seed": 0,
        "preferred": preferred,
        "lines": lines,
        "weights": weights,
    });

    let id = format!("{:x}", {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        task_bias_id.hash(&mut h);
        step_bias_id.hash(&mut h);
        h.finish()
    });

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;

    conn.execute(
        "INSERT OR REPLACE INTO bias_artifacts
         (id, task_bias_id, step_bias_id, created_at, artifact_type, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            task_bias_id,
            step_bias_id,
            now,
            "semantic_bias_v1",
            serde_json::to_string(&payload)?
        ],
    )?;

    println!("ok\t{id}");
    Ok(())
}

// -- subcommand: latest-bias-artifact ----------------------------------------

fn cmd_latest_bias_artifact(args: &[String]) -> Result<()> {
    use rusqlite::Connection;

    if args.len() < 2 {
        bail!("latest-bias-artifact <task_bias_id> <step_bias_id>");
    }
    let task_bias_id = &args[0];
    let step_bias_id = &args[1];

    let db_path = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| "kernel.db".to_string());

    let conn = Connection::open(&db_path)?;

    let mut stmt = conn.prepare(
        "SELECT id, task_bias_id, step_bias_id, created_at, artifact_type, payload
         FROM bias_artifacts
         WHERE task_bias_id = ?1 AND step_bias_id = ?2
         ORDER BY created_at DESC LIMIT 1",
    )?;

    let row = stmt
        .query_row(rusqlite::params![task_bias_id, step_bias_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(|_| anyhow::anyhow!("no artifact found for {task_bias_id}/{step_bias_id}"))?;

    println!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        row.0, row.1, row.2, row.3, row.4, row.5
    );
    Ok(())
}

// -- subcommand: analyze-task -------------------------------------------------

fn cmd_analyze_task(args: &[String]) -> Result<()> {
    let mut payload = None;
    let mut seed: Option<u64> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--payload" => {
                i += 1;
                payload = Some(args[i].clone());
            }
            "--seed" => {
                i += 1;
                seed = Some(args[i].parse()?);
            }
            other => bail!("unknown argument: {other}"),
        }
        i += 1;
    }
    let payload = payload.ok_or_else(|| anyhow::anyhow!("--payload required"))?;
    let seed = seed.unwrap_or(0);

    let ctx = PipelineContext {
        seed,
        bias_version: BiasVersion::V1,
    };
    let pipeline = Pipeline::new(default_bias());
    let out = pipeline.run(payload, &ctx)?;

    println!("plan_id : {}", out.plan.id);
    println!("steps   : {}", out.plan.steps.len());
    for s in &out.plan.steps {
        println!("  - {s}");
    }
    Ok(())
}

// -- subcommand: doctor-json --------------------------------------------------

fn cmd_doctor_json(_args: &[String]) -> Result<()> {
    use serde_json::json;

    // Disk free space (root partition)
    let free_gb: f64 = {
        #[cfg(unix)]
        {
            use std::mem::MaybeUninit;
            let mut stat: libc::statvfs = unsafe { MaybeUninit::zeroed().assume_init() };
            let path = std::ffi::CString::new("/").unwrap();
            if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } == 0 {
                (stat.f_bavail as f64 * stat.f_frsize as f64) / 1_073_741_824.0
            } else {
                -1.0
            }
        }
        #[cfg(not(unix))]
        {
            -1.0
        }
    };

    // MLX runtime probe via OPENAI_BASE_URL/v1/models
    let (mlx_runtime_ready, mlx_models) = deterministic_ai_kernel::lm_control::probe_mlx_runtime();
    let mlx_model_present = deterministic_ai_kernel::lm_control::model_path_present();

    // Roles — read from ROLES_MANIFEST_PATH or empty
    let roles: Vec<serde_json::Value> = {
        let manifest = std::env::var("ROLES_MANIFEST_PATH")
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Vec<serde_json::Value>>(&s).ok())
            .unwrap_or_default();
        manifest
            .into_iter()
            .map(|r| {
                let role = r["role"].as_str().unwrap_or("unknown").to_owned();
                let manifest_model = r["manifest_model"].as_str().unwrap_or("").to_owned();
                let env_model =
                    std::env::var(format!("{}_MODEL", role.to_uppercase().replace('-', "_")))
                        .unwrap_or_default();
                let in_sync = manifest_model == env_model && !manifest_model.is_empty();
                let model_available =
                    mlx_runtime_ready && mlx_models.iter().any(|m| m == &manifest_model);
                let threshold_gb = r["threshold_gb"].as_f64().unwrap_or(4.0);
                let switch_ready = in_sync && model_available && free_gb >= threshold_gb;
                json!({
                    "role": role,
                    "manifest_model": manifest_model,
                    "env_model": env_model,
                    "in_sync": in_sync,
                    "model_available": model_available,
                    "threshold_gb": threshold_gb,
                    "switch_ready": switch_ready,
                })
            })
            .collect()
    };

    let report = json!({
        "command": "doctor-json",
        "ok": true,
        "schema_version": "cli-json-v1",
        "report": {
            "free_gb": free_gb,
            "mlx_models": mlx_models,
            "mlx_runtime_ready": mlx_runtime_ready,
            "mlx_model_present": mlx_model_present,
            "roles": roles,
        }
    });

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[allow(dead_code)]
fn dirs_next_or_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
}
