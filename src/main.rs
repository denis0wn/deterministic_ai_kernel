mod ai;
mod api;
mod cli_json;
mod effects;
mod embeddings;
mod event_bus;
mod execution;
mod kernel_types;
mod leases;
mod llm;
mod lm_control;
mod model_manifest;
mod model_registry;
mod replay;
mod scheduler;
mod schema;
mod snapshot;
mod worker;
mod workflow;

use cli_json::emit_json;
use effects::execute_effects;
use execution::runtime::Runtime;
use leases::{expire_leases, seed_demo_leases};
use replay::capsule::build_replay_capsule;
use replay::engine::replay_validate;
use rusqlite::Connection;
use scheduler::{current_status_map, next_ready_step, reconcile, schedule};
use snapshot::{rebuild_snapshot, restore_snapshot};
use std::fs;
use workflow::compiler::Workflow;

fn suppress_nested_cargo_warnings() {
    if std::env::var_os("RUSTFLAGS").is_none() {
        unsafe {
            std::env::set_var("RUSTFLAGS", "-Awarnings");
        }
    }
}

fn print_stats(db: &str) {
    let conn = Connection::open(db).unwrap();

    let events: i64 = conn
        .query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0))
        .unwrap_or(0);

    let causal_units: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT causal_unit_id) FROM event_log",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let max_generation: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(system_generation), 0) FROM event_log",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let tasks: i64 = conn
        .query_row("SELECT COUNT(DISTINCT task_id) FROM event_log", [], |r| {
            r.get(0)
        })
        .unwrap_or(0);

    println!("EVENTS: {}", events);
    println!("CAUSAL_UNITS: {}", causal_units);
    println!("MAX_GENERATION: {}", max_generation);
    println!("TASKS: {}", tasks);
}

fn table_exists(db: &str, table: &str) -> bool {
    use rusqlite::{Connection, OpenFlags};

    if !std::path::Path::new(db).exists() {
        return false;
    }

    let conn = match Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(c) => c,
        Err(_) => return false,
    };

    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1 LIMIT 1",
        [table],
        |_r| Ok(()),
    )
    .is_ok()
}

fn reset_db(db: &str) {
    if !std::path::Path::new(db).exists() {
        println!("RESET OK");
        return;
    }

    let conn = Connection::open(db).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA wal_checkpoint(FULL);
        DELETE FROM event_log;
        DELETE FROM effect_ledger;
        DELETE FROM step_dependencies;
        DELETE FROM step_status;
        DELETE FROM state_snapshots;
        DELETE FROM generations;
        VACUUM;
        "#,
    )
    .unwrap();

    println!("RESET OK");
}

fn integrity_json_report(db: &str) -> serde_json::Value {
    use serde_json::{json, Value};
    use std::fs;

    if std::path::Path::new(db).exists() {
        let _ = fs::remove_file(db);
        let _ = fs::remove_file(format!("{db}-wal"));
        let _ = fs::remove_file(format!("{db}-shm"));
    }

    let conn = Connection::open(db).unwrap();
    conn.execute_batch(include_str!("../event_bus/schema.sql"))
        .unwrap();
    drop(conn);

    snapshot::rebuild_snapshot(db, "integrity-task", true).unwrap();
    snapshot::restore_snapshot(db, "integrity-task", true).unwrap();

    let conn = Connection::open(db).unwrap();
    let payload: String = conn
        .query_row(
            "SELECT payload FROM state_snapshots WHERE task_id = ?1 ORDER BY snapshot_id DESC LIMIT 1",
            ["integrity-task"],
            |r| r.get(0),
        )
        .unwrap();

    let parsed: Value = serde_json::from_str(&payload).unwrap();

    json!({
        "ok": true,
        "snapshot_version": parsed.get("snapshot_version").and_then(|v| v.as_u64()).unwrap_or(0),
        "schema_version": parsed.get("schema_version").and_then(|v| v.as_u64()).unwrap_or(0),
        "created_at_present": parsed.get("created_at").and_then(|v| v.as_u64()).is_some(),
        "state_hash_present": parsed.get("state_hash").and_then(|v| v.as_u64()).is_some(),
        "state_present": parsed.get("state").and_then(|v| v.as_object()).is_some(),
        "task_id": parsed.get("task_id").cloned().unwrap_or(Value::Null)
    })
}

fn run_integrity(db: &str) {
    let report = integrity_json_report(db);

    assert_eq!(report.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        report.get("snapshot_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        report.get("schema_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        report.get("created_at_present").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        report.get("state_hash_present").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        report.get("state_present").and_then(|v| v.as_bool()),
        Some(true)
    );

    println!("INTEGRITY OK");
}

fn run_integrity_json(db: &str) {
    let report = crate::api::integrity_json_report(db);
    emit_json("integrity-json", report);
}

fn vacuum_db(db: &str) {
    if !std::path::Path::new(db).exists() {
        println!("VACUUM OK");
        return;
    }

    let conn = Connection::open(db).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA wal_checkpoint(FULL);
        VACUUM;
        "#,
    )
    .unwrap();

    println!("VACUUM OK");
}

#[tokio::main]
async fn main() {
    suppress_nested_cargo_warnings();
    model_registry::validate().unwrap();
    let args: Vec<String> = std::env::args().collect();
    let db = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap()
            .join("kernel.db")
            .to_string_lossy()
            .to_string()
    });
    let db = db.as_str();

    if matches!(
        args.get(1).map(|s| s.as_str()),
        Some("--help") | Some("-h") | Some("help")
    ) {
        println!("deterministic_ai_kernel commands:");
        println!("  llm-smoke");
        println!("  embeddings-smoke");
        println!("  llm-planner-smoke");
        println!("  print-model-manifest");
        println!("  current-models");
        println!("  latest-bias-artifact <task_id> [step_id]");
        println!("  semantic-artifacts <task_id> [step_id]");
        println!("  capture-capsule <task_id>");
        println!("  capture-capsule-save <task_id> [--json]");
        println!("  latest-capsule <task_id>");
        println!("  replay-capsule <task_id> [--json]");
        println!("  compare-capsules <task_id_a> <task_id_b> [--explain] [--json]");
        println!("  bias-explain <step_kind>...");
        println!("  analyze-task <task_id> <text>");
        println!("  pipeline-run --task-id <id> [--seed <u64>] [--json]");
        println!("  pipeline-run --payload <text> [--seed <u64>] [--json]");
        println!("  doctor");
        println!("  doctor-json");
        println!("  integrity");
        println!("  integrity-json");
        println!("  auto-route <coding_assistant|task_planning|embeddings>");
        println!("  switch <coding_assistant|task_planning|embeddings> [--dry-run]");
        println!("  sync-all-model-roles");
        println!("  claim-worker [task_id] [worker_id]");
        println!("  start-step [task_id] [worker_id] <step_id>");
        println!("  heartbeat [task_id] [worker_id] <step_id>");
        println!("  fail-step [task_id] [worker_id] <step_id> <reason>");
        println!("  complete-step [task_id] [worker_id] <step_id>");
        println!("  rmdb");
        return;
    }

    match args.get(1).map(|s| s.as_str()) {
        Some("capture-capsule-save") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            let json_output = args.iter().any(|a| a == "--json");
            if task_id.trim().is_empty() {
                eprintln!("usage: capture-capsule-save <task_id> [--json]");
                std::process::exit(1);
            }

            if json_output {
                match crate::api::capture_capsule_save_json(db, &task_id) {
                    Ok(report) => emit_json("capture-capsule-save", report),
                    Err(e) => {
                        eprintln!("capture-capsule-save failed: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match crate::api::capture_capsule_save_text(db, &task_id) {
                    Ok(report) => {
                        println!(
                            "CAPTURE_CAPSULE_SAVE_OK[id]{}[id]{}[id]events={}[id]nodes={}[id]edges={}",
                            report.execution_id,
                            report.capsule_id,
                            report.events,
                            report.nodes,
                            report.edges
                        );
                    }
                    Err(e) => {
                        eprintln!("capture-capsule-save failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            return;
        }

        Some("compare-capsules") => {
            let left = args.get(2).cloned().unwrap_or_default();
            let right = args.get(3).cloned().unwrap_or_default();
            let explain = args.iter().any(|a| a == "--explain");
            let json_output = args.iter().any(|a| a == "--json");

            if left.trim().is_empty() || right.trim().is_empty() {
                eprintln!("usage: compare-capsules <task_id_a> <task_id_b> [--explain] [--json]");
                std::process::exit(1);
            }

            if json_output {
                match crate::api::compare_capsules_json(db, &left, &right) {
                    Ok(report) => emit_json("compare-capsules", report),
                    Err(e) => {
                        eprintln!("compare-capsules failed: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match crate::api::compare_capsules_text(db, &left, &right) {
                    Ok(report) => {
                        if explain {
                            println!(
                                "COMPARE_CAPSULES_OK[id]{}[id]{}[id]status={}[id]explanation={}",
                                report.left_capsule_id,
                                report.right_capsule_id,
                                report.status,
                                report.explanation
                            );
                        } else {
                            println!(
                                "COMPARE_CAPSULES_OK[id]{}[id]{}[id]status={}",
                                report.left_capsule_id, report.right_capsule_id, report.status
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("compare-capsules failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            return;
        }

        Some("replay-capsule") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            let json_output = args.iter().any(|a| a == "--json");
            if task_id.trim().is_empty() {
                eprintln!("usage: replay-capsule <task_id> [--json]");
                std::process::exit(1);
            }

            if json_output {
                match crate::api::replay_capsule_json(db, &task_id) {
                    Ok(report) => emit_json("replay-capsule", report),
                    Err(e) => {
                        eprintln!("replay-capsule failed: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match crate::api::replay_capsule_text(db, &task_id) {
                    Ok(report) => {
                        println!(
                            "REPLAY_CAPSULE_OK[id]{}[id]{}[id]events={}[id]nodes={}[id]edges={}[id]valid={}",
                            report.execution_id,
                            report.capsule_id,
                            report.events,
                            report.nodes,
                            report.edges,
                            report.valid
                        );
                    }
                    Err(e) => {
                        eprintln!("replay-capsule failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            return;
        }

        Some("latest-capsule") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            if task_id.trim().is_empty() {
                eprintln!("usage: latest-capsule <task_id>");
                std::process::exit(1);
            }

            let bus = event_bus::EventBus::new(db).unwrap();
            match bus.latest_replay_capsule(&task_id) {
                Ok(Some(capsule)) => {
                    println!("{}", serde_json::to_string_pretty(&capsule).unwrap());
                }
                Ok(None) => {
                    eprintln!("no replay capsule found for task_id={}", task_id);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("latest-capsule failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }

        Some("capture-capsule") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            if task_id.trim().is_empty() {
                eprintln!("usage: capture-capsule <task_id>");
                std::process::exit(1);
            }

            let bus = event_bus::EventBus::new(db).unwrap();
            match build_replay_capsule(&bus, &task_id) {
                Ok(capsule) => {
                    println!("{}", serde_json::to_string_pretty(&capsule).unwrap());
                }
                Err(e) => {
                    eprintln!("capture-capsule failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }

        Some("emit-bias-artifact") => {
            use serde_json::json;
            use workflow::contract::StepKind;
            use workflow::semantic::bias::SemanticBias;

            fn parse_step_kind(s: &str) -> Option<StepKind> {
                match s {
                    "TightenPlannerPrompt" => Some(StepKind::TightenPlannerPrompt),
                    "NormalizePlannerOutput" => Some(StepKind::NormalizePlannerOutput),
                    "AddLlmFallbackHandling" => Some(StepKind::AddLlmFallbackHandling),
                    "AddPlannerTestCoverage" => Some(StepKind::AddPlannerTestCoverage),
                    "ValidatePlannerOutput" => Some(StepKind::ValidatePlannerOutput),
                    "AnalyzeTask" => Some(StepKind::AnalyzeTask),
                    "PlanExecution" => Some(StepKind::PlanExecution),
                    "ExecuteChanges" => Some(StepKind::ExecuteChanges),
                    "ReadRepository" => Some(StepKind::ReadRepository),
                    "LocateBug" => Some(StepKind::LocateBug),
                    "PatchCode" => Some(StepKind::PatchCode),
                    "RunTests" => Some(StepKind::RunTests),
                    "ValidatePatch" => Some(StepKind::ValidatePatch),
                    _ => None,
                }
            }

            let task_id = args.get(2).cloned().unwrap_or_default();
            let step_id = args.get(3).cloned().unwrap_or_default();
            if task_id.trim().is_empty() || step_id.trim().is_empty() {
                eprintln!("usage: emit-bias-artifact <task_id> <step_id> <step_kind>...");
                std::process::exit(1);
            }

            let raw_domain: Vec<String> = args.iter().skip(4).cloned().collect();
            if raw_domain.is_empty() {
                eprintln!("usage: emit-bias-artifact <task_id> <step_id> <step_kind>...");
                std::process::exit(1);
            }

            let mut domain: Vec<StepKind> = Vec::new();
            for raw in &raw_domain {
                match parse_step_kind(raw) {
                    Some(kind) => domain.push(kind),
                    None => {
                        eprintln!("unknown step kind: {raw}");
                        std::process::exit(2);
                    }
                }
            }

            let bias = SemanticBias::neutral_for(&domain);
            let payload = json!({
                "lines": bias.explain_lines(),
                "version": bias.version,
                "seed": bias.seed,
                "preferred": bias.preferred.iter().map(|k| format!("{:?}", k)).collect::<Vec<_>>(),
                "weights": bias.weights,
            });

            let bus = event_bus::EventBus::new(db).unwrap();
            match bus.append_semantic_artifact(&task_id, &step_id, 0, "semantic_bias_v1", &payload)
            {
                Ok(()) => {
                    println!("EMIT_BIAS_ARTIFACT_OK[id]{}[id]{}", task_id, step_id);
                }
                Err(e) => {
                    eprintln!("emit-bias-artifact failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }

        Some("bias-explain") => {
            use workflow::contract::StepKind;
            use workflow::semantic::bias::SemanticBias;

            fn parse_step_kind(s: &str) -> Option<StepKind> {
                match s {
                    "TightenPlannerPrompt" => Some(StepKind::TightenPlannerPrompt),
                    "NormalizePlannerOutput" => Some(StepKind::NormalizePlannerOutput),
                    "AddLlmFallbackHandling" => Some(StepKind::AddLlmFallbackHandling),
                    "AddPlannerTestCoverage" => Some(StepKind::AddPlannerTestCoverage),
                    "ValidatePlannerOutput" => Some(StepKind::ValidatePlannerOutput),
                    "AnalyzeTask" => Some(StepKind::AnalyzeTask),
                    "PlanExecution" => Some(StepKind::PlanExecution),
                    "ExecuteChanges" => Some(StepKind::ExecuteChanges),
                    "ReadRepository" => Some(StepKind::ReadRepository),
                    "LocateBug" => Some(StepKind::LocateBug),
                    "PatchCode" => Some(StepKind::PatchCode),
                    "RunTests" => Some(StepKind::RunTests),
                    "ValidatePatch" => Some(StepKind::ValidatePatch),
                    _ => None,
                }
            }

            let raw_domain: Vec<String> = args.iter().skip(2).cloned().collect();
            if raw_domain.is_empty() {
                eprintln!("usage: bias-explain <step_kind>...");
                std::process::exit(2);
            }

            let mut domain: Vec<StepKind> = Vec::new();
            for raw in &raw_domain {
                match parse_step_kind(raw) {
                    Some(kind) => domain.push(kind),
                    None => {
                        eprintln!("unknown step kind: {raw}");
                        std::process::exit(2);
                    }
                }
            }

            let bias = SemanticBias::neutral_for(&domain);
            for line in bias.explain_lines() {
                println!("{line}");
            }
            return;
        }
        Some("llm-smoke") => {
            match llm::smoke().await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("llm-smoke failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("embeddings-smoke") => {
            match embeddings::embeddings_smoke().await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("embeddings-smoke failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }

        Some("llm-planner-smoke") => {
            match llm::planner_smoke().await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("llm-planner-smoke failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("print-model-manifest") => {
            match model_manifest::print_manifest() {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("print-model-manifest failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("current-models") => {
            match model_manifest::print_current_models() {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("current-models failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("latest-bias-artifact") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            if task_id.trim().is_empty() {
                eprintln!("usage: cargo run -- latest-bias-artifact <task_id> [step_id]");
                std::process::exit(1);
            }
            let step_id = args.get(3).map(|s| s.as_str());
            let bus = event_bus::EventBus::new(db).unwrap();
            match bus.list_semantic_artifacts(&task_id, step_id) {
                Ok(rows) => {
                    if let Some(row) = rows
                        .into_iter()
                        .find(|r| r.artifact_type == "semantic_bias_v1")
                    {
                        println!(
                            "{}	{}	{}	{}	{}	{}",
                            row.artifact_id,
                            row.task_id,
                            row.step_id,
                            row.source_generation,
                            row.artifact_type,
                            row.payload
                        );
                    }
                }
                Err(e) => {
                    eprintln!("latest-bias-artifact failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }

        Some("semantic-artifacts") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            if task_id.trim().is_empty() {
                eprintln!("usage: cargo run -- semantic-artifacts <task_id> [step_id]");
                std::process::exit(1);
            }
            let step_id = args.get(3).map(|s| s.as_str());
            let bus = event_bus::EventBus::new(db).unwrap();
            match bus.list_semantic_artifacts(&task_id, step_id) {
                Ok(rows) => {
                    for row in rows {
                        println!(
                            "{}[id]{}[id]{}[id]{}[id]{}[id]{}",
                            row.artifact_id,
                            row.task_id,
                            row.step_id,
                            row.source_generation,
                            row.artifact_type,
                            row.payload
                        );
                    }
                }
                Err(e) => {
                    eprintln!("semantic-artifacts failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }

        Some("latest-analysis-seed") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            if task_id.trim().is_empty() {
                eprintln!("usage: cargo run -- latest-analysis-seed <task_id> [step_id]");
                std::process::exit(1);
            }
            let step_id = args.get(3).map(|s| s.as_str());
            let bus = event_bus::EventBus::new(db).unwrap();
            match bus.latest_analysis_seed(&task_id, step_id) {
                Ok(Some(row)) => {
                    println!(
                        "{}	{}	{}	{}	{}	{}",
                        row.artifact_id,
                        row.task_id,
                        row.step_id,
                        row.source_generation,
                        row.artifact_type,
                        row.payload
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("latest-analysis-seed failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }

        Some("analyze-task") => {
            let task_id = args.get(2).cloned().unwrap_or_default();
            if task_id.trim().is_empty() {
                eprintln!("usage: cargo run -- analyze-task <task_id> <text>");
                std::process::exit(1);
            }

            let detail = args.iter().skip(3).cloned().collect::<Vec<_>>().join(" ");
            if detail.trim().is_empty() {
                eprintln!("usage: cargo run -- analyze-task <task_id> <text>");
                std::process::exit(1);
            }

            let conn = rusqlite::Connection::open(db).unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO tasks (task_id, task_class) VALUES (?1, 'Generic')",
                rusqlite::params![task_id],
            )
            .unwrap();
            drop(conn);

            let bus = event_bus::EventBus::new(db).unwrap();
            let runtime = Runtime::new(bus);

            let step = workflow::contract::Step {
                kind: workflow::contract::StepKind::AnalyzeTask,
                detail: Some(detail),
            };

            match runtime.execute_step(&task_id, &step).await {
                Ok(()) => {
                    println!("ANALYZE_TASK_OK task_id={}", task_id);
                }
                Err(e) => {
                    eprintln!("analyze-task failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }

        Some("doctor") => {
            match lm_control::print_doctor_text() {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("doctor failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("integrity") => {
            run_integrity(db);
            return;
        }
        Some("integrity-json") => {
            run_integrity_json(db);
            return;
        }
        Some("doctor-json") => {
            match crate::api::doctor_json() {
                Ok(report) => emit_json("doctor-json", report),
                Err(e) => {
                    eprintln!("doctor-json failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("auto-route") => {
            let role = args.get(2).cloned().unwrap_or_default();
            if role.trim().is_empty() {
                eprintln!(
                    "usage: cargo run -- auto-route <coding_assistant|task_planning|embeddings>"
                );
                std::process::exit(1);
            }

            match lm_control::auto_route(&role) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("auto-route failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("sync-all-model-roles") => {
            match model_manifest::sync_all_roles() {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("sync-all-model-roles failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("memory") => {
            let threshold = args.get(2).and_then(|s| s.parse::<f64>().ok());
            match lm_control::print_memory(threshold) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("memory failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("switch") => {
            let role = args.get(2).cloned().unwrap_or_default();
            if role.trim().is_empty() {
                eprintln!("usage: cargo run -- switch <coding_assistant|task_planning|embeddings> [--dry-run]");
                std::process::exit(1);
            }

            let dry_run = args.iter().any(|a| a == "--dry-run");
            let result = if dry_run {
                lm_control::dry_run_switch(&role)
            } else {
                lm_control::safe_switch(&role)
            };

            match result {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("switch failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("llm-prompt") => {
            let prompt = args.get(2..).map(|xs| xs.join(" ")).unwrap_or_default();
            if prompt.trim().is_empty() {
                eprintln!("usage: cargo run -- llm-prompt [id]your prompt here[id]");
                std::process::exit(1);
            }

            let text = llm::coding_assistant(&prompt).await.unwrap();

            println!("{}", text);
            return;
        }
        Some("prompt") => {
            let role = args.get(2).cloned().unwrap_or_default();
            let text = args.get(3..).map(|xs| xs.join(" ")).unwrap_or_default();
            if role.trim().is_empty() || text.trim().is_empty() {
                eprintln!("usage: dak prompt <role> [id]your text here[id]");
                std::process::exit(1);
            }
            match lm_control::send_prompt(&role, &text).await {
                Ok(response) => println!("{}", response),
                Err(e) => {
                    eprintln!("prompt failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("gateway-stdin") => {
            use std::io::Read;

            let mut input = String::new();
            std::io::stdin().read_to_string(&mut input).unwrap();

            if input.trim().is_empty() {
                eprintln!("gateway-stdin: empty stdin");
                std::process::exit(1);
            }

            let parsed: serde_json::Value = match serde_json::from_str(&input) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("gateway-stdin: invalid json: {}", e);
                    std::process::exit(1);
                }
            };

            println!("{}", serde_json::to_string(&parsed).unwrap());
            return;
        }
        Some("pipeline-run") => {
            let mut task_id: Option<String> = None;
            let mut payload: Option<String> = None;
            let mut seed: u64 = 42;
            let mut as_json = false;
            let mut i = 2usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--task-id" => {
                        i += 1;
                        task_id = args.get(i).cloned();
                    }
                    "--payload" => {
                        i += 1;
                        payload = args.get(i).cloned();
                    }
                    "--seed" => {
                        i += 1;
                        seed = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(42);
                    }
                    "--json" => {
                        as_json = true;
                    }
                    other => {
                        eprintln!("unknown arg: {other}");
                        std::process::exit(1);
                    }
                }
                i += 1;
            }
            if task_id.is_some() == payload.is_some() {
                eprintln!("usage: pipeline-run --task-id <id> [--seed <u64>] [--json]");
                eprintln!("   or: pipeline-run --payload [id]...[id] [--seed <u64>] [--json]");
                std::process::exit(1);
            }
            let resolved = if let Some(p) = payload {
                p
            } else {
                let conn = rusqlite::Connection::open(db).unwrap();
                let id = task_id.unwrap();
                conn.query_row(
                    "SELECT input_representation FROM semantic_bias_artifacts WHERE task_id = ?1 ORDER BY artifact_id DESC LIMIT 1",
                    [&id], |r| r.get::<_, String>(0)
                ).unwrap_or_else(|_| { eprintln!("pipeline-run: no payload for task_id={id}"); std::process::exit(1); })
            };
            let report = match deterministic_ai_kernel::planner_pipeline::build_plan_and_publish(
                &resolved, seed, db,
            ) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("pipeline-run failed: {e}");
                    std::process::exit(1);
                }
            };
            let task_id = report.plan.id.clone();
            std::fs::create_dir_all("artifacts").unwrap();
            std::fs::write(
                format!("artifacts/pipeline_input.{}.txt", task_id),
                &resolved,
            )
            .unwrap();
            {
                let conn = rusqlite::Connection::open(db).unwrap();
                conn.execute(
                    "INSERT OR IGNORE INTO tasks (task_id, task_class) VALUES (?1, 'Generic')",
                    rusqlite::params![task_id.as_str()],
                )
                .unwrap();
            }
            if let Err(e) = schedule(db, &task_id) {
                eprintln!("schedule failed: {e}");
                std::process::exit(1);
            }
            if let Err(e) = execute_effects(db, &task_id) {
                eprintln!("execute-effects failed: {e}");
                std::process::exit(1);
            }

            let final_answer_path = format!(
                "{}/artifacts/final_answer.{}.txt",
                std::env::current_dir().unwrap().display(),
                task_id
            );
            let final_answer = std::fs::read_to_string(&final_answer_path).unwrap_or_else(|_| {
                let fallback = resolved.trim().to_string();
                let _ = std::fs::create_dir_all(format!(
                    "{}/artifacts",
                    std::env::current_dir().unwrap().display()
                ));
                let _ = std::fs::write(&final_answer_path, &fallback);
                fallback
            });

            if as_json {
                let out = serde_json::json!({
                    "plan_id": report.plan.id,
                    "seed": report.plan.seed,
                    "steps": report.plan.steps,
                    "fingerprint": report.fingerprint,
                    "planner_version": report.planner_version,
                    "elapsed_ms": report.elapsed_ms,
                    "final_answer": final_answer,
                    "critic": { "passed": report.critic_report.passed, "warnings": report.critic_report.warnings, "violations": report.critic_report.invariant_violations },
                    "stage_events": report.stage_events.iter().map(|e| serde_json::json!({"stage": e.stage.to_string(), "offset_ms": e.timestamp_offset_ms, "desc": e.description})).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                println!("PLAN_ID={}", report.plan.id);
                println!("PLANNER_VERSION={}", report.planner_version);
                println!("SEED={}", report.plan.seed);
                println!("STEP_COUNT={}", report.plan.steps.len());
                println!("FINGERPRINT={}", report.fingerprint);
                println!("ELAPSED_MS={}", report.elapsed_ms);
                println!("CRITIC_PASSED={}", report.critic_report.passed);
                println!("FINAL_ANSWER={}", final_answer.trim());
                for (i, s) in report.plan.steps.iter().enumerate() {
                    println!("STEP.{}={}", i + 1, s);
                }
                for e in &report.stage_events {
                    println!(
                        "STAGE|{}|{}ms|{}",
                        e.stage, e.timestamp_offset_ms, e.description
                    );
                }
            }
            return;
        }
        Some("plan-task") => {
            let task = args.get(2..).map(|xs| xs.join(" ")).unwrap_or_default();
            if task.trim().is_empty() {
                eprintln!("usage: cargo run -- plan-task [id]your task here[id]");
                eprintln!(
                    "   or: cargo run -- plan-task --planner-hardening [id]your task here[id]"
                );
                eprintln!("   or: cargo run -- plan-task --compile-error path/to/log.txt");
                eprintln!("   or: cargo run -- plan-task --test-failure path/to/log.txt");
                eprintln!("   or: cargo run -- plan-task --lint-report path/to/log.txt");
                std::process::exit(1);
            }

            let input = if let Some(rest) = task.strip_prefix("--planner-hardening ") {
                workflow::compiler::TaskInput::planner_hardening(rest)
            } else if let Some(rest) = task.strip_prefix("--compile-error ") {
                workflow::compiler::TaskInput::from_compile_error(rest)
            } else if let Some(rest) = task.strip_prefix("--test-failure ") {
                workflow::compiler::TaskInput::from_test_failure(rest)
            } else if let Some(rest) = task.strip_prefix("--lint-report ") {
                workflow::compiler::TaskInput::from_lint_report(rest)
            } else {
                workflow::compiler::TaskInput::generic(task.as_str())
            };

            let steps = Workflow::build_from_task_llm(&input).await.unwrap();

            for step in steps {
                println!("{}", step.as_text());
            }
            return;
        }
        Some("reset") => {
            reset_db(db);
            return;
        }
        Some("replay") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let ok = replay_validate(db, task_id);
            println!("REPLAY OK: {}", ok);
            return;
        }
        Some("stats") => {
            print_stats(db);
            return;
        }
        Some("status-map") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let status = current_status_map(db, task_id).unwrap();
            println!("STEP_STATUS: {:?}", status);
            return;
        }
        Some("next-ready") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let step = next_ready_step(db, task_id).unwrap();
            match step {
                Some(step_id) => println!("NEXT_READY: {}", step_id),
                None => println!("NEXT_READY: <none>"),
            }
            return;
        }
        Some("vacuum") => {
            vacuum_db(db);
            return;
        }
        Some("snapshot") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            if !table_exists(db, "event_log") || !table_exists(db, "state_snapshots") {
                println!("SNAPSHOT OK");
                return;
            }
            rebuild_snapshot(db, task_id, false).unwrap();
            return;
        }
        Some("restore") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            if !table_exists(db, "state_snapshots") {
                println!("RESTORE OK");
                return;
            }
            restore_snapshot(db, task_id, false).unwrap();
            return;
        }
        Some("snapshot-artifacts") => {
            use rusqlite::Connection;
            use serde_json::Value;

            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            if !table_exists(db, "state_snapshots") {
                return;
            }

            let conn = Connection::open(db).unwrap();
            let payload: String = conn.query_row(
                "SELECT payload FROM state_snapshots WHERE task_id = ?1 ORDER BY snapshot_id DESC LIMIT 1",
                [task_id],
                |r| r.get(0),
            ).unwrap();

            let payload_json: Value = serde_json::from_str(&payload).unwrap();
            if let Some(artifacts) = payload_json.get("artifacts").and_then(|v| v.as_object()) {
                for (artifact_type, artifact_id) in artifacts {
                    println!("ARTIFACT_REF\t{}\t{}", artifact_type, artifact_id);
                }
            }
            return;
        }
        Some("schedule") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            schedule(db, task_id).unwrap();
            return;
        }
        Some("reconcile") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            reconcile(db, task_id).unwrap();
            return;
        }
        Some("execute-effects") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            execute_effects(db, task_id).unwrap();
            return;
        }
        Some("seed-leases") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            seed_demo_leases(db, task_id).unwrap();
            return;
        }
        Some("expire-leases") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            expire_leases(db, task_id).unwrap();
            return;
        }
        Some("submit-task") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            {
                let conn = rusqlite::Connection::open(db).unwrap();
                conn.execute(
                    "INSERT OR IGNORE INTO tasks (task_id, task_class) VALUES (?1, 'Generic')",
                    rusqlite::params![task_id],
                )
                .unwrap();
            }
            scheduler::schedule(db, task_id).unwrap();
            return;
        }
        Some("claim-worker") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            worker::claim_worker(db, task_id, worker_id).unwrap();
            return;
        }
        Some("complete-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            worker::complete_step(db, task_id, worker_id, step_id).unwrap();
            scheduler::schedule(db, task_id).unwrap();
            return;
        }
        Some("fail-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            let reason = args.get(5).map(|s| s.as_str()).unwrap_or("worker_error");
            worker::fail_step(db, task_id, worker_id, step_id, reason).unwrap();
            return;
        }
        Some("start-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            worker::start_step(db, task_id, worker_id, step_id).unwrap();
            return;
        }
        Some("heartbeat") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            worker::heartbeat(db, task_id, worker_id, step_id).unwrap();
            return;
        }
        Some("rmdb") => {
            let _ = fs::remove_file(db);
            println!("RMDB OK");
            return;
        }
        Some("models-list") => {
            crate::lm_control::print_all_models_v0()
                .unwrap_or_else(|e| eprintln!("models-list: {e}"));
            return;
        }
        Some("models-loaded") => {
            crate::lm_control::print_loaded_models()
                .unwrap_or_else(|e| eprintln!("models-loaded: {e}"));
            return;
        }
        Some("model-load") => {
            let id = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() {
                eprintln!("usage: model-load <model-id>");
                return;
            }
            crate::lm_control::load_model(id).unwrap_or_else(|e| eprintln!("model-load: {e}"));
            return;
        }
        Some("model-unload") => {
            let id = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() {
                eprintln!("usage: model-unload <model-id>");
                return;
            }
            crate::lm_control::unload_model(id).unwrap_or_else(|e| eprintln!("model-unload: {e}"));
            return;
        }
        Some("smart-switch") => {
            let id = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let gb = args
                .get(3)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(4.0);
            if id.is_empty() {
                eprintln!("usage: smart-switch <model-id> [required_gb]");
                return;
            }
            crate::lm_control::smart_switch(id, gb)
                .unwrap_or_else(|e| eprintln!("smart-switch: {e}"));
            return;
        }
        _ => {}
    }

    eprintln!("no command provided");
    eprintln!("run: cargo run -- --help");
    std::process::exit(1);
}

#[cfg(test)]
mod integrity_json_tests {
    use super::*;

    #[test]
    fn integrity_json_report_has_expected_shape() {
        let db = std::env::temp_dir()
            .join(format!(
                "deterministic_ai_kernel_integrity_json_{}.db",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .to_string_lossy()
            .to_string();

        let report = cli_json::command_report("integrity-json", integrity_json_report(&db));

        assert_eq!(report.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            report.get("schema_version").and_then(|v| v.as_str()),
            Some("cli-json-v1")
        );
        assert_eq!(
            report.get("command").and_then(|v| v.as_str()),
            Some("integrity-json")
        );
        assert!(report.get("report").is_some());
        assert!(report["report"].get("snapshot_version").is_some());
        assert!(report["report"].get("schema_version").is_some());
        assert!(report["report"].get("created_at_present").is_some());
        assert!(report["report"].get("state_hash_present").is_some());
        assert!(report["report"].get("state_present").is_some());
        assert!(report["report"].get("task_id").is_some());

        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(format!("{db}-wal"));
        let _ = std::fs::remove_file(format!("{db}-shm"));
    }
}
