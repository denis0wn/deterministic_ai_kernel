mod effects;
mod embeddings;
mod event_bus;
mod execution;
mod leases;
mod llm;
mod lm_control;
mod model_registry;
mod model_manifest;
mod replay;
mod scheduler;
mod snapshot;
mod worker;
mod workflow;

use effects::execute_effects;
use execution::runtime::Runtime;
use leases::{expire_leases, seed_demo_leases};
use replay::engine::replay_validate;
use rusqlite::Connection;
use scheduler::{current_status_map, next_ready_step, reconcile, schedule};
use snapshot::{rebuild_snapshot, restore_snapshot};
use std::fs;
use workflow::compiler::Workflow;

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
    model_registry::validate().unwrap();
    let args: Vec<String> = std::env::args().collect();
    let db = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| "kernel.db".to_string());
    let db = db.as_str();

    if matches!(args.get(1).map(|s| s.as_str()), Some("--help") | Some("-h") | Some("help")) {
        println!("deterministic_ai_kernel commands:");
        println!("  llm-smoke");
        println!("  embeddings-smoke");
        println!("  llm-planner-smoke");
        println!("  print-model-manifest");
        println!("  current-models");
        println!("  semantic-artifacts <task_id> [step_id]");
        println!("  analyze-task <task_id> <text>");
        println!("  doctor");
        println!("  doctor-json");
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
                    eprintln!("semantic-artifacts failed: {e}");
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
        Some("doctor-json") => {
            match lm_control::print_doctor_json() {
                Ok(()) => {}
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
                eprintln!("usage: cargo run -- auto-route <coding_assistant|task_planning|embeddings>");
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
                eprintln!("usage: cargo run -- llm-prompt \"your prompt here\"");
                std::process::exit(1);
            }

            let text = llm::coding_assistant(&prompt).await.unwrap();

            println!("{}", text);
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
        Some("plan-task") => {
            let task = args.get(2..).map(|xs| xs.join(" ")).unwrap_or_default();
            if task.trim().is_empty() {
                eprintln!("usage: cargo run -- plan-task \"your task here\"");
                eprintln!("   or: cargo run -- plan-task --planner-hardening \"your task here\"");
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
            rebuild_snapshot(db, task_id).unwrap();
            return;
        }
        Some("restore") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            restore_snapshot(db, task_id).unwrap();
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
        _ => {}
    }


    eprintln!("no command provided");
    eprintln!("run: cargo run -- --help");
    std::process::exit(1);
}
