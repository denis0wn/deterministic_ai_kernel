use deterministic_ai_kernel::cli_json::emit_json;
use deterministic_ai_kernel::effects::execute_effects;
use deterministic_ai_kernel::leases::{expire_leases, seed_demo_leases};
use deterministic_ai_kernel::providers::storage::StorageProvider;
use deterministic_ai_kernel::replay::capsule::build_replay_capsule;
use deterministic_ai_kernel::replay::engine::replay_validate;
use deterministic_ai_kernel::scheduler::{
    current_status_map, next_ready_step, reconcile, schedule,
};
use deterministic_ai_kernel::snapshot::{rebuild_snapshot, restore_snapshot};
use deterministic_ai_kernel::workflow::compiler::Workflow;
use std::fs;

/// Unwrap a kernel operation at the CLI boundary without panicking: any
/// error becomes a clean domain error message and a non-zero exit code.
/// Fresh/invalid databases must never crash the CLI with a Rust panic
/// (audit finding H1).
fn cli_expect<T>(op: &str, result: anyhow::Result<T>) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{} failed: {}", op, e);
            std::process::exit(1);
        }
    }
}

fn print_stats(db: &str) {
    let (events, causal_units, max_generation, tasks) =
        deterministic_ai_kernel::providers::storage_for(db)
            .print_stats()
            .unwrap_or_else(|e| {
                eprintln!("stats failed: {e}");
                std::process::exit(1);
            });

    println!("EVENTS: {}", events);
    println!("CAUSAL_UNITS: {}", causal_units);
    println!("MAX_GENERATION: {}", max_generation);
    println!("TASKS: {}", tasks);
}

fn table_exists(db: &str, table: &str) -> bool {
    // Preserve the old behavior: a missing db file is "no table" — never
    // create/initialize one as a side effect of asking.
    if !std::path::Path::new(db).exists() {
        return false;
    }
    deterministic_ai_kernel::providers::storage_for(db).table_exists(table)
}

fn reset_db(db: &str) {
    if !std::path::Path::new(db).exists() {
        println!("RESET OK");
        return;
    }

    // The full 13-table reset (audit finding M2) lives in the storage
    // layer — no SQL in the CLI.
    deterministic_ai_kernel::providers::storage_for(db)
        .reset_db()
        .unwrap_or_else(|e| {
            deterministic_ai_kernel::kernel_error::exit_with_error(
                &deterministic_ai_kernel::kernel_error::CliError::Database(format!(
                    "reset_db execution failed: {e}"
                )),
            );
        });

    println!("RESET OK");
}

fn integrity_json_report(db: &str) -> serde_json::Value {
    deterministic_ai_kernel::api::integrity_json_report(db).unwrap_or_else(|e| {
        deterministic_ai_kernel::kernel_error::exit_with_error(
            &deterministic_ai_kernel::kernel_error::CliError::Database(format!(
                "integrity_json_report failed: {e}"
            )),
        )
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

fn run_integrity_json(db: &str) -> anyhow::Result<()> {
    let report = deterministic_ai_kernel::api::integrity_json_report(db)?;
    emit_json("integrity-json", report)?;
    Ok(())
}

fn vacuum_db(db: &str) {
    if !std::path::Path::new(db).exists() {
        println!("VACUUM OK");
        return;
    }

    deterministic_ai_kernel::providers::storage_for(db)
        .vacuum_db()
        .unwrap_or_else(|e| {
            deterministic_ai_kernel::kernel_error::exit_with_error(
                &deterministic_ai_kernel::kernel_error::CliError::Database(format!(
                    "VACUUM failed: {e}"
                )),
            );
        });

    println!("VACUUM OK");
}

/// PROGRESS UNTIL VERIFIED stage 3: record the terminal taxonomy
/// assessment for the task's fingerprint group as an observation-only
/// TASK_TERMINAL_ASSESSED event. Never fatal — a failed assessment
/// must not mask the real task outcome.
fn record_terminal_assessment(db: &str, task_id: &str) {
    let conn = match deterministic_ai_kernel::providers::storage::open_initialized(db) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("progress: terminal assessment could not open db: {e}");
            return;
        }
    };
    match deterministic_ai_kernel::progress::terminal_assessment_for_task(
        &conn,
        std::path::Path::new("artifacts"),
        task_id,
    ) {
        Ok(Some((fp, taxonomy, basis))) => {
            let payload = serde_json::json!({
                "task_id": task_id,
                "payload_fingerprint": fp,
                "taxonomy": serde_json::to_value(&taxonomy)
                    .unwrap_or(serde_json::json!("InProgress")),
                "basis": basis,
                "policy": "progress_until_verified/stage3",
            });
            if let Err(e) = deterministic_ai_kernel::providers::storage_for(db).append_event(
                task_id,
                None,
                "TASK_TERMINAL_ASSESSED",
                &payload,
            ) {
                eprintln!("progress: failed to record TASK_TERMINAL_ASSESSED: {e}");
            }
            println!("TERMINAL_ASSESSMENT: {taxonomy:?} ({basis})");
        }
        Ok(None) => {}
        Err(e) => eprintln!("progress: terminal assessment failed: {e}"),
    }
}

#[tokio::main]
async fn main() {
    // Model-registry validation happens at point of use (resolve_model is
    // fail-closed on missing env). Validating unconditionally at startup made
    // every subcommand — including pure ones like bias-explain, --help and
    // integrity-json — panic on hosts without OPENAI_BASE_URL/OPENAI_MODEL,
    // which is exactly the CI environment (.env is gitignored).
    // Loading .env (no validation) restores the ambient config side effect
    // that paths like doctor/model_path_present rely on.
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    let db = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|e| {
                eprintln!("Failed to get current directory: {e}");
                std::process::exit(1);
            })
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
        println!("  lm-lifecycle <status|stop|start>");
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
                match deterministic_ai_kernel::api::capture_capsule_save_json(db, &task_id) {
                    Ok(report) => {
                        let _ = emit_json("capture-capsule-save", report);
                    }
                    Err(e) => {
                        eprintln!("capture-capsule-save failed: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match deterministic_ai_kernel::api::capture_capsule_save_text(db, &task_id) {
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
                match deterministic_ai_kernel::api::compare_capsules_json(db, &left, &right) {
                    Ok(report) => {
                        let _ = emit_json("compare-capsules", report);
                    }
                    Err(e) => {
                        eprintln!("compare-capsules failed: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match deterministic_ai_kernel::api::compare_capsules_text(db, &left, &right) {
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
                match deterministic_ai_kernel::api::replay_capsule_json(db, &task_id) {
                    Ok(report) => {
                        let _ = emit_json("replay-capsule", report);
                    }
                    Err(e) => {
                        eprintln!("replay-capsule failed: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match deterministic_ai_kernel::api::replay_capsule_text(db, &task_id) {
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

            let bus = deterministic_ai_kernel::event_bus::EventBus::new(db).unwrap();
            match bus.latest_replay_capsule(&task_id) {
                Ok(Some(capsule)) => {
                    let _ = emit_json("latest-capsule", serde_json::to_value(&capsule).unwrap());
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

            let bus = deterministic_ai_kernel::event_bus::EventBus::new(db).unwrap();
            match build_replay_capsule(&bus, &task_id) {
                Ok(capsule) => {
                    let _ = emit_json("capture-capsule", serde_json::to_value(&capsule).unwrap());
                }
                Err(e) => {
                    eprintln!("capture-capsule failed: {e}");
                    std::process::exit(1);
                }
            }
            return;
        }

        Some("emit-bias-artifact") => {
            use deterministic_ai_kernel::workflow::contract::StepKind;
            use deterministic_ai_kernel::workflow::semantic::bias::SemanticBias;
            use serde_json::json;

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

            let bus = deterministic_ai_kernel::event_bus::EventBus::new(db).unwrap();
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
            use deterministic_ai_kernel::workflow::contract::StepKind;
            use deterministic_ai_kernel::workflow::semantic::bias::SemanticBias;

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
            match deterministic_ai_kernel::llm::smoke().await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("llm-smoke failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("__lifecycle-supervise") => {
            // P4-C: hidden entrypoint run as a detached supervisor so that
            // ephemeral CLI processes still get deterministic idle unload.
            // Not part of the operator surface (not listed in help).
            std::process::exit(deterministic_ai_kernel::mlx_lifecycle::run_supervisor_loop());
        }
        Some("lm-lifecycle") => {
            // P0 MLX lifecycle operator controls: status | stop | start.
            let action = args.get(2).cloned().unwrap_or_else(|| "status".to_string());
            let lc = deterministic_ai_kernel::mlx_lifecycle::global();
            match action.as_str() {
                "status" => {
                    let base = std::env::var("OPENAI_BASE_URL")
                        .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_string());
                    // Prime base_url so endpoint probing has a target.
                    let _ = deterministic_ai_kernel::lm_control::probe_mlx_runtime();
                    let st = lc.status();
                    println!("STATE={:?}", st.state);
                    println!("ENABLED={}", st.enabled);
                    println!(
                        "PID={}",
                        st.pid
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "none".to_string())
                    );
                    println!("OWNED={}", st.owned);
                    println!("EXTERNAL={}", st.external);
                    println!(
                        "ENDPOINT_UP={}",
                        deterministic_ai_kernel::mlx_lifecycle::probe_endpoint(&base)
                    );
                    println!("IDLE_SECS={}", st.idle_secs);
                    println!("TIMEOUT_SECS={}", st.timeout_secs);
                    println!("IN_FLIGHT={}", st.in_flight);
                    if let Some(f) = st.last_failure {
                        println!("LAST_FAILURE={f}");
                    }
                    // Cross-process truth: what the persisted lifecycle state
                    // file says (written by the owning process).
                    if let Some(p) = deterministic_ai_kernel::mlx_lifecycle::persisted_snapshot() {
                        println!("PERSISTED_STATE={:?}", p.state);
                        println!(
                            "PERSISTED_PID={}",
                            p.pid
                                .map(|x| x.to_string())
                                .unwrap_or_else(|| "none".to_string())
                        );
                        println!("PERSISTED_OWNED={}", p.owned);
                        println!("LAST_ACTIVITY_UNIX={}", p.last_activity_unix);
                        println!(
                            "NOW_UNIX={}",
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        );
                    }
                }
                "stop" => match deterministic_ai_kernel::mlx_lifecycle::stop_managed_server() {
                    Ok(report) => {
                        println!(
                            "SHUTDOWN attempted={} pid={} escalated={} process_gone={} endpoint_down={} verified={} detail={}",
                            report.attempted,
                            report.pid.map(|p| p.to_string()).unwrap_or_else(|| "none".to_string()),
                            report.escalated_to_kill,
                            report.process_gone,
                            report.endpoint_down,
                            report.verified(),
                            report.detail
                        );
                        if report.attempted && !report.verified() {
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("lm-lifecycle stop failed: {e}");
                        std::process::exit(1);
                    }
                },
                "start" => {
                    let base = std::env::var("OPENAI_BASE_URL")
                        .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_string());
                    match deterministic_ai_kernel::mlx_lifecycle::ensure_ready(&base) {
                        Ok(()) => {
                            let st = lc.status();
                            println!(
                                "STARTED state={:?} pid={}",
                                st.state,
                                st.pid
                                    .map(|p| p.to_string())
                                    .unwrap_or_else(|| "none".to_string())
                            );
                        }
                        Err(e) => {
                            eprintln!("lm-lifecycle start failed: {e}");
                            std::process::exit(1);
                        }
                    }
                }
                "watch" => {
                    // Resident lifecycle holder: ensure the server is up, then
                    // keep this process alive so the idle watcher can perform
                    // the IDLE_TIMEOUT -> GRACEFUL_SHUTDOWN -> MODEL_UNLOADED
                    // transition. Exits once a kernel-owned server has been
                    // unloaded; runs indefinitely when timeout=0.
                    let base = std::env::var("OPENAI_BASE_URL")
                        .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_string());
                    if let Err(e) = deterministic_ai_kernel::mlx_lifecycle::ensure_ready(&base) {
                        eprintln!("lm-lifecycle watch failed to ensure server: {e}");
                        std::process::exit(1);
                    }
                    let st = lc.status();
                    println!(
                        "WATCHING state={:?} pid={} owned={} external={} timeout_secs={}",
                        st.state,
                        st.pid
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "none".to_string()),
                        st.owned,
                        st.external,
                        st.timeout_secs
                    );
                    if st.external {
                        println!("NOTE: server is external; lifecycle will never unload it");
                    }
                    let mut was_loaded = st.owned && !st.external;
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        let st = lc.status();
                        if was_loaded
                            && st.state
                                == deterministic_ai_kernel::mlx_lifecycle::LifecycleState::Unloaded
                        {
                            println!("MODEL_UNLOADED verified");
                            return;
                        }
                        if st.owned && !st.external {
                            was_loaded = true;
                        }
                    }
                }
                other => {
                    eprintln!("lm-lifecycle: unknown action '{other}' (use status|stop|start)");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("embeddings-smoke") => {
            match deterministic_ai_kernel::embeddings::embeddings_smoke().await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("embeddings-smoke failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }

        Some("llm-planner-smoke") => {
            match deterministic_ai_kernel::llm::planner_smoke().await {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("llm-planner-smoke failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("print-model-manifest") => {
            match deterministic_ai_kernel::model_manifest::print_manifest() {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("print-model-manifest failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("current-models") => {
            match deterministic_ai_kernel::model_manifest::print_current_models() {
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
            let bus = deterministic_ai_kernel::event_bus::EventBus::new(db).unwrap();
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
            let bus = deterministic_ai_kernel::event_bus::EventBus::new(db).unwrap();
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
            let bus = deterministic_ai_kernel::event_bus::EventBus::new(db).unwrap();
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

            let storage = deterministic_ai_kernel::providers::storage_for(db);
            storage
                .insert_task(&task_id, "Generic", "")
                .unwrap_or_else(|e| {
                    eprintln!("Failed to insert task: {e}");
                    std::process::exit(1);
                });
            // Persist the input representation so `pipeline-run --task-id`
            // can resolve it. ANALYZE_TASK_OK is only printed after the
            // payload is durably stored (audit finding C6: the previous
            // implementation printed OK after a silent no-op).
            storage
                .insert_semantic_bias_artifact(&task_id, &detail)
                .unwrap_or_else(|e| {
                    eprintln!("Failed to store input representation: {e}");
                    std::process::exit(1);
                });
            println!("ANALYZE_TASK_OK task_id={}", task_id);
            return;
        }

        Some("doctor") => {
            match deterministic_ai_kernel::lm_control::print_doctor_text() {
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
            run_integrity_json(db).unwrap_or_else(|e| {
                eprintln!("integrity-json failed: {e}");
                std::process::exit(1);
            });
            return;
        }
        Some("doctor-json") => {
            match deterministic_ai_kernel::api::doctor_json() {
                Ok(report) => {
                    let _ = emit_json("doctor-json", report);
                }
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

            match deterministic_ai_kernel::lm_control::auto_route(&role) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("auto-route failed: {e}");
                    std::process::exit(1);
                }
            };
            return;
        }
        Some("sync-all-model-roles") => {
            match deterministic_ai_kernel::model_manifest::sync_all_roles() {
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
            match deterministic_ai_kernel::lm_control::print_memory(threshold) {
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
                deterministic_ai_kernel::lm_control::dry_run_switch(&role)
            } else {
                deterministic_ai_kernel::lm_control::safe_switch(&role)
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

            let text = deterministic_ai_kernel::llm::coding_assistant(&prompt)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Coding assistant failed: {e}");
                    std::process::exit(1);
                });

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
            match deterministic_ai_kernel::lm_control::send_prompt(&role, &text).await {
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

            println!(
                "{}",
                serde_json::to_string(&parsed).unwrap_or_else(|e| {
                    eprintln!("Serialization failed: {e}");
                    std::process::exit(1);
                })
            );
            return;
        }
        Some("pipeline-run") => {
            let mut task_id: Option<String> = None;
            let mut payload: Option<String> = None;
            let mut seed: u64 = 42;
            let mut as_json = false;
            // Stage 4 decomposition: operator-declared lemma registration.
            let mut subtask_of: Option<String> = None;
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
                    "--subtask-of" => {
                        i += 1;
                        subtask_of = args.get(i).cloned();
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
            // C2: stamp the kernel seed onto this process so every model
            // call carries it on the wire and into recorded llm_calls.
            std::env::set_var("DAK_KERNEL_SEED", seed.to_string());
            let resolved = if let Some(p) = payload {
                p
            } else {
                let id = task_id.unwrap_or_else(|| {
                    eprintln!("task_id is missing");
                    std::process::exit(1);
                });
                deterministic_ai_kernel::providers::storage_for(db)
                    .get_semantic_bias_payload(&id)
                    .unwrap_or_else(|_| {
                        eprintln!("pipeline-run: no payload for task_id={id}");
                        std::process::exit(1);
                    })
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
            .unwrap_or_else(|e| {
                eprintln!("Failed to write pipeline input: {e}");
                std::process::exit(1);
            });
            // PROGRESS UNTIL VERIFIED — stage 2 (detect-only): if this exact
            // payload already failed with a byte-identical failure
            // signature, record a REPETITION observation event. Detection
            // never blocks execution in stage 2; strategy selection is a
            // later stage.
            match deterministic_ai_kernel::providers::storage::open_initialized(db) {
                Ok(conn) => {
                    match deterministic_ai_kernel::progress::prior_failure_matches(
                        &conn,
                        std::path::Path::new("artifacts"),
                        &resolved,
                    ) {
                        Ok(Some((prior_task, reason))) => {
                            let strategies =
                                deterministic_ai_kernel::strategy::admissible_strategies(&reason);
                            let repetition_payload = serde_json::json!({
                                "task_id": task_id,
                                "repeats_task": prior_task,
                                "payload_fingerprint":
                                    deterministic_ai_kernel::progress::payload_fingerprint(&resolved),
                                "failure_signature": reason,
                                "admissible_strategies": strategies,
                                "policy": "progress_until_verified/stage3_detect_only",
                            });
                            if let Err(e) = deterministic_ai_kernel::providers::storage_for(db)
                                .append_event(&task_id, None, "REPETITION", &repetition_payload)
                            {
                                eprintln!("progress: failed to record REPETITION event: {e}");
                            }
                            println!("NOTE    : REPETITION DETECTED — this payload previously failed with an identical signature in task {prior_task}.");
                            println!("NOTE    : Admissible strategies (kernel-enumerated): {strategies:?}. Re-running without new information or a new strategy is not progress (stage 3: detect-only, execution proceeds).");
                        }
                        Ok(None) => {}
                        Err(e) => eprintln!("progress: repetition check failed: {e}"),
                    }
                }
                Err(e) => eprintln!("progress: repetition check could not open db: {e}"),
            }
            {
                // Persist the planner's canonical ExecSpec with the task.
                // The scheduler must execute the planned graph, not a
                // regenerated TaskClass default; the spec is only filled in
                // if the task row has none (first publish wins).
                let spec_json = serde_json::to_string(&report.plan.spec).unwrap_or_else(|e| {
                    eprintln!("Failed to serialize plan spec: {e}");
                    std::process::exit(1);
                });
                // Deterministic task-intent classification (P0, H-2 fix):
                // interrogative plans are stored as Question tasks instead of
                // the former hardcoded 'Generic' literal. Classification is
                // kernel-owned; the LLM is never consulted here.
                let task_class_str =
                    match deterministic_ai_kernel::workflow::planner::classify_task_class(
                        &report.plan.steps,
                    ) {
                        deterministic_ai_kernel::workflow::contract::TaskClass::Question => {
                            "Question"
                        }
                        _ => "Generic",
                    };
                deterministic_ai_kernel::providers::storage_for(db)
                    .upsert_task_exec_spec(task_id.as_str(), task_class_str, &spec_json)
                    .unwrap_or_else(|e| {
                        eprintln!("Failed to insert task: {e}");
                        std::process::exit(1);
                    });
            }
            // PROGRESS UNTIL VERIFIED stage 4: operator-declared lemma
            // registration. Kernel-owned event: only this CLI path can
            // create subtask status, and only before dispatch. The
            // completion gate reads it; the taxonomy excludes subtasks
            // from VerifiedSuccess (lemmas are not theorems).
            if let Some(carrier_label) = &subtask_of {
                let subtask_payload = serde_json::json!({
                    "task_id": task_id,
                    "carrier_label": carrier_label,
                    "role": "lemma",
                    "composition_contract": "semantic verification delegated to the composition carrier's task-level tests",
                    "policy": "progress_until_verified/stage4",
                });
                if let Err(e) = deterministic_ai_kernel::providers::storage_for(db).append_event(
                    &task_id,
                    None,
                    "SUBTASK_OF",
                    &subtask_payload,
                ) {
                    eprintln!("progress: failed to record SUBTASK_OF: {e}");
                    std::process::exit(1);
                }
                println!("NOTE    : task {task_id} registered as decomposition subtask (lemma) of carrier '{carrier_label}'; semantic verification delegated to the carrier.");
            }
            if let Err(e) = schedule(db, &task_id) {
                eprintln!("schedule failed: {e}");
                std::process::exit(1);
            }
            if let Err(e) = execute_effects(db, &task_id) {
                // R4 (HD-3): a failed task must surface a terminal STATE line
                // on stdout — previously the process exited with only an
                // eprintln, so harnesses saw an empty TASK_STATE and read it
                // as a silent stall.
                println!("TASK_STATE: failed");
                println!("TASK_FAILED_REASON: {e}");
                record_terminal_assessment(db, &task_id);
                eprintln!("execute-effects failed: {e}");
                std::process::exit(1);
            }
            record_terminal_assessment(db, &task_id);

            let final_answer_path = format!(
                "{}/artifacts/final_answer.{}.txt",
                std::env::current_dir()
                    .unwrap_or_else(|e| {
                        eprintln!("Failed to get current directory: {e}");
                        std::process::exit(1);
                    })
                    .display(),
                task_id
            );
            let final_answer = std::fs::read_to_string(&final_answer_path).unwrap_or_else(|_| {
                let fallback = resolved.trim().to_string();
                let _ = std::fs::create_dir_all(format!(
                    "{}/artifacts",
                    std::env::current_dir()
                        .unwrap_or_else(|e| {
                            eprintln!("Failed to get current directory: {e}");
                            std::process::exit(1);
                        })
                        .display()
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
                let _ = deterministic_ai_kernel::cli_json::emit_json("pipeline-run", out);
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
                deterministic_ai_kernel::workflow::compiler::TaskInput::planner_hardening(rest)
            } else if let Some(rest) = task.strip_prefix("--compile-error ") {
                deterministic_ai_kernel::workflow::compiler::TaskInput::from_compile_error(rest)
            } else if let Some(rest) = task.strip_prefix("--test-failure ") {
                deterministic_ai_kernel::workflow::compiler::TaskInput::from_test_failure(rest)
            } else if let Some(rest) = task.strip_prefix("--lint-report ") {
                deterministic_ai_kernel::workflow::compiler::TaskInput::from_lint_report(rest)
            } else {
                deterministic_ai_kernel::workflow::compiler::TaskInput::generic(task.as_str())
            };

            let steps = Workflow::build_from_task_llm(&input)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Workflow compile failed: {e}");
                    std::process::exit(1);
                });

            // Persist the compiled plan as the canonical ExecSpec so the
            // scheduler executes exactly these steps and dependencies
            // (planner output == persisted spec == scheduler graph).
            let task_class = input.task_class();
            let class_name = match task_class {
                deterministic_ai_kernel::workflow::contract::TaskClass::Generic => "Generic",
                deterministic_ai_kernel::workflow::contract::TaskClass::PlannerHardening => {
                    "PlannerHardening"
                }
                deterministic_ai_kernel::workflow::contract::TaskClass::CodeFix => "CodeFix",
                deterministic_ai_kernel::workflow::contract::TaskClass::Question => "Question",
            };
            let spec = deterministic_ai_kernel::workflow::contract::steps_to_exec_spec(&steps);
            let spec_json = serde_json::to_string(&spec).unwrap_or_else(|e| {
                eprintln!("Failed to serialize exec spec: {e}");
                std::process::exit(1);
            });
            let task_id = format!(
                "task-{}",
                &blake3::hash(format!("{}:{}", class_name, task.trim()).as_bytes()).to_hex()[..12]
            );

            deterministic_ai_kernel::providers::storage_for(db)
                .upsert_task_exec_spec(&task_id, class_name, &spec_json)
                .unwrap_or_else(|e| {
                    eprintln!("Failed to insert task: {e}");
                    std::process::exit(1);
                });

            println!("TASK_ID: {}", task_id);
            println!("TASK_CLASS: {}", class_name);
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
            // OPS-1: the library validator is silent; the CLI owns its own
            // output — a clean verdict plus any violations on stderr. The
            // effect counts stay part of the CLI contract (golden corpus).
            let storage = deterministic_ai_kernel::providers::storage_for(db);
            for v in storage.replay_violations(task_id).unwrap_or_default() {
                eprintln!("{v}");
            }
            let ok = replay_validate(db, task_id);
            let (committed, rejected) = deterministic_ai_kernel::providers::storage_for(db)
                .effect_ledger_counts(task_id)
                .unwrap_or((0, 0));
            println!("COMMITTED_EFFECTS: {committed}");
            println!("REJECTED_EFFECTS: {rejected}");
            println!("REPLAY {}", if ok { "VALID" } else { "INVALID" });
            println!("REPLAY OK: {}", ok);
            return;
        }
        Some("verifier-gap") => {
            // PROGRESS UNTIL VERIFIED stage 5: verifier-gap lifecycle.
            //   verifier-gap prove   --task <id> --criterion <text> --uncovered <text>
            //   verifier-gap grant   --task <id> --verifier-file <path>
            //   verifier-gap decline --task <id> --reason <text>
            // The kernel verifies the BASIS (attempt ledger, inventory,
            // lifecycle) from its own data; criterion/uncovered are
            // human-stated and recorded as such. The LLM's "I cannot
            // verify" is never accepted as a gap. Grant/decline are
            // human decisions; the kernel records them and shapes the
            // terminal taxonomy (granted => resume; declined =>
            // constructive NVPF).
            let action = args.get(2).cloned().unwrap_or_default();
            let mut task_id: Option<String> = None;
            let mut criterion: Option<String> = None;
            let mut uncovered: Option<String> = None;
            let mut verifier_file: Option<String> = None;
            let mut reason: Option<String> = None;
            let mut i = 3usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--task" => {
                        i += 1;
                        task_id = args.get(i).cloned();
                    }
                    "--criterion" => {
                        i += 1;
                        criterion = args.get(i).cloned();
                    }
                    "--uncovered" => {
                        i += 1;
                        uncovered = args.get(i).cloned();
                    }
                    "--verifier-file" => {
                        i += 1;
                        verifier_file = args.get(i).cloned();
                    }
                    "--reason" => {
                        i += 1;
                        reason = args.get(i).cloned();
                    }
                    other => {
                        eprintln!("unknown arg: {other}");
                        std::process::exit(1);
                    }
                }
                i += 1;
            }
            let task_id = task_id.unwrap_or_else(|| {
                eprintln!("verifier-gap: --task is required");
                std::process::exit(1);
            });
            let conn = deterministic_ai_kernel::providers::storage::open_initialized(db)
                .unwrap_or_else(|e| {
                    eprintln!("verifier-gap: failed to open db: {e}");
                    std::process::exit(1);
                });
            let store = deterministic_ai_kernel::providers::storage_for(db);
            match action.as_str() {
                "prove" => {
                    let (criterion, uncovered) = match (criterion, uncovered) {
                        (Some(c), Some(u)) => (c, u),
                        _ => {
                            eprintln!(
                                "verifier-gap prove: --criterion and --uncovered are required"
                            );
                            std::process::exit(1);
                        }
                    };
                    let proof = deterministic_ai_kernel::progress::gap_proof_payload(
                        &conn,
                        std::path::Path::new("artifacts"),
                        &task_id,
                        &criterion,
                        &uncovered,
                    )
                    .unwrap_or_else(|e| {
                        eprintln!("verifier-gap: proof construction failed: {e}");
                        std::process::exit(1);
                    })
                    .unwrap_or_else(|| {
                        eprintln!("verifier-gap: task '{task_id}' has no attempts — a gap proof requires an attempt basis");
                        std::process::exit(1);
                    });
                    if let Err(e) = store.append_event(&task_id, None, "VERIFIER_GAP_PROOF", &proof)
                    {
                        eprintln!("verifier-gap: failed to record gap proof: {e}");
                        std::process::exit(1);
                    }
                    println!("VERIFIER GAP PROOF RECORDED for task {task_id}:");
                    println!("  criterion (human-stated)  : {criterion}");
                    println!("  uncovered (human-stated)  : {uncovered}");
                    println!(
                        "  verifier inventory        : {} primitives",
                        deterministic_ai_kernel::progress::verifier_inventory().len()
                    );
                    println!("  status                    : pending human grant/decline");
                }
                "grant" => {
                    if deterministic_ai_kernel::progress::gap_status(&conn, &task_id)
                        != deterministic_ai_kernel::progress::GapStatus::Pending
                    {
                        eprintln!("verifier-gap grant: no PENDING gap proof for task '{task_id}'");
                        std::process::exit(1);
                    }
                    let path = verifier_file.unwrap_or_else(|| {
                        eprintln!("verifier-gap grant: --verifier-file is required");
                        std::process::exit(1);
                    });
                    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
                        eprintln!("verifier-gap grant: cannot read verifier file '{path}': {e}");
                        std::process::exit(1);
                    });
                    let file_hash = blake3::hash(&bytes).to_hex().to_string();
                    let payload = serde_json::json!({
                        "task_id": task_id,
                        "verifier_path": path,
                        "verifier_blake3": file_hash,
                        "source": "human-authored",
                        "note": "kernel records provenance only; authority comes from human review",
                        "policy": "progress_until_verified/stage5",
                    });
                    if let Err(e) = store.append_event(&task_id, None, "VERIFIER_GRANTED", &payload)
                    {
                        eprintln!("verifier-gap: failed to record grant: {e}");
                        std::process::exit(1);
                    }
                    println!("VERIFIER GRANTED for task {task_id}: {path} (blake3 {file_hash}). Resume with a new attempt.");
                }
                "decline" => {
                    if deterministic_ai_kernel::progress::gap_status(&conn, &task_id)
                        != deterministic_ai_kernel::progress::GapStatus::Pending
                    {
                        eprintln!(
                            "verifier-gap decline: no PENDING gap proof for task '{task_id}'"
                        );
                        std::process::exit(1);
                    }
                    let reason = reason.unwrap_or_else(|| {
                        eprintln!("verifier-gap decline: --reason is required");
                        std::process::exit(1);
                    });
                    let payload = serde_json::json!({
                        "task_id": task_id,
                        "reason": reason,
                        "decided_by": "human",
                        "policy": "progress_until_verified/stage5",
                    });
                    if let Err(e) =
                        store.append_event(&task_id, None, "VERIFIER_DECLINED", &payload)
                    {
                        eprintln!("verifier-gap: failed to record decline: {e}");
                        std::process::exit(1);
                    }
                    println!("VERIFIER GAP DECLINED for task {task_id}: {reason}");
                    println!("Terminal taxonomy: NO VERIFIED PATH FOUND (constructive; full ledger recorded).");
                }
                other => {
                    eprintln!("verifier-gap: unknown action '{other}' (prove|grant|decline)");
                    std::process::exit(1);
                }
            }
            return;
        }
        Some("decomposition") => {
            // PROGRESS UNTIL VERIFIED stage 4: finalize a decomposition
            // record once the composition carrier has completed: emits
            // TASK_DECOMPOSED on the carrier naming its subtask(s) and
            // the composition contract. Observation-only.
            // usage: decomposition --carrier <task_id> --subtask <task_id>
            //        --target-file <path> [--acceptance <text>]
            let mut carrier: Option<String> = None;
            let mut subtask: Option<String> = None;
            let mut target_file: Option<String> = None;
            let mut acceptance = "carrier task-level tests".to_string();
            let mut i = 2usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--carrier" => {
                        i += 1;
                        carrier = args.get(i).cloned();
                    }
                    "--subtask" => {
                        i += 1;
                        subtask = args.get(i).cloned();
                    }
                    "--target-file" => {
                        i += 1;
                        target_file = args.get(i).cloned();
                    }
                    "--acceptance" => {
                        i += 1;
                        if let Some(a) = args.get(i) {
                            acceptance = a.clone();
                        }
                    }
                    other => {
                        eprintln!("unknown arg: {other}");
                        std::process::exit(1);
                    }
                }
                i += 1;
            }
            let (carrier, subtask, target_file) = match (carrier, subtask, target_file) {
                (Some(c), Some(s), Some(t)) => (c, s, t),
                _ => {
                    eprintln!(
                        "usage: decomposition --carrier <task_id> --subtask <task_id> --target-file <path> [--acceptance <text>]"
                    );
                    std::process::exit(1);
                }
            };
            let storage = deterministic_ai_kernel::providers::storage_for(db);
            for (name, id) in [("carrier", &carrier), ("subtask", &subtask)] {
                let exists = storage.task_exists(id).unwrap_or_else(|e| {
                    eprintln!("decomposition: task lookup failed: {e}");
                    std::process::exit(1);
                });
                if !exists {
                    eprintln!("decomposition: {name} task '{id}' does not exist");
                    std::process::exit(1);
                }
            }
            let payload = serde_json::json!({
                "task_id": carrier,
                "subtasks": [subtask],
                "composition_contract": {
                    "target_file": target_file,
                    "regions": "disjoint (each subtask patch grounded exactly once at its apply moment)",
                    "acceptance": acceptance,
                },
                "policy": "progress_until_verified/stage4",
            });
            if let Err(e) = deterministic_ai_kernel::providers::storage_for(db).append_event(
                &carrier,
                None,
                "TASK_DECOMPOSED",
                &payload,
            ) {
                eprintln!("decomposition: failed to record TASK_DECOMPOSED: {e}");
                std::process::exit(1);
            }
            println!("DECOMPOSITION RECORDED: carrier {carrier} <- subtask {subtask} (target {target_file}; acceptance: {acceptance})");
            return;
        }
        Some("progress") => {
            // PROGRESS UNTIL VERIFIED — stage 2 ledger: attempts grouped
            // by payload fingerprint with progress/repetition verdicts.
            // Detect-only; prints the ledger and exits 0.
            let conn = deterministic_ai_kernel::providers::storage::open_initialized(db)
                .unwrap_or_else(|e| {
                    eprintln!("progress: failed to open db: {e}");
                    std::process::exit(1);
                });
            let report = deterministic_ai_kernel::progress::assess_db(
                &conn,
                std::path::Path::new("artifacts"),
            )
            .unwrap_or_else(|e| {
                eprintln!("progress: assessment failed: {e}");
                std::process::exit(1);
            });
            print!("{}", deterministic_ai_kernel::progress::render(&report));
            return;
        }
        Some("stats") => {
            print_stats(db);
            return;
        }
        Some("status-map") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let status = cli_expect("status-map", current_status_map(db, task_id));
            println!("STEP_STATUS: {:?}", status);
            return;
        }
        Some("next-ready") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let step = cli_expect("next-ready", next_ready_step(db, task_id));
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
            cli_expect("snapshot", rebuild_snapshot(db, task_id, false));
            return;
        }
        Some("restore") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            if !table_exists(db, "state_snapshots") {
                println!("RESTORE OK");
                return;
            }
            cli_expect("restore", restore_snapshot(db, task_id, false));
            return;
        }
        Some("snapshot-artifacts") => {
            use serde_json::Value;

            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            if !table_exists(db, "state_snapshots") {
                return;
            }

            let payload: String = match deterministic_ai_kernel::providers::storage_for(db)
                .get_latest_snapshot_payload(task_id)
            {
                Ok(Some(p)) => p,
                _ => {
                    eprintln!("snapshot-artifacts: no snapshot for task_id={}", task_id);
                    std::process::exit(1);
                }
            };

            let payload_json: Value = match serde_json::from_str(&payload) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("snapshot-artifacts: malformed snapshot payload: {e}");
                    std::process::exit(1);
                }
            };
            if let Some(artifacts) = payload_json.get("artifacts").and_then(|v| v.as_object()) {
                for (artifact_type, artifact_id) in artifacts {
                    println!("ARTIFACT_REF\t{}\t{}", artifact_type, artifact_id);
                }
            }
            return;
        }
        Some("schedule") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            cli_expect("schedule", schedule(db, task_id));
            return;
        }
        Some("reconcile") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            cli_expect("reconcile", reconcile(db, task_id));
            return;
        }
        Some("execute-effects") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            cli_expect("execute-effects", execute_effects(db, task_id));
            return;
        }
        Some("seed-leases") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            cli_expect("seed-demo-leases", seed_demo_leases(db, task_id));
            return;
        }
        Some("expire-leases") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            cli_expect("expire-leases", expire_leases(db, task_id));
            return;
        }
        Some("submit-task") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            // Write the default flow explicitly: tasks must carry an
            // ExecSpec (the TaskClass read-time fallback is removed).
            let spec_json = serde_json::to_string(
                &deterministic_ai_kernel::workflow::contract::TaskClass::Generic.to_exec_spec(None),
            )
            .unwrap_or_else(|e| {
                eprintln!("submit-task: spec serialize failed: {e}");
                std::process::exit(1);
            });
            deterministic_ai_kernel::providers::storage_for(db)
                .insert_task(task_id, "Generic", &spec_json)
                .unwrap_or_else(|e| {
                    eprintln!("submit-task failed: {e}");
                    std::process::exit(1);
                });
            cli_expect(
                "schedule",
                deterministic_ai_kernel::scheduler::schedule(db, task_id),
            );
            return;
        }
        Some("claim-worker") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            cli_expect(
                "claim-worker",
                deterministic_ai_kernel::worker::claim_worker(db, task_id, worker_id),
            );
            return;
        }
        Some("complete-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            cli_expect(
                "complete-step",
                deterministic_ai_kernel::worker::complete_step(db, task_id, worker_id, step_id),
            );
            cli_expect(
                "schedule",
                deterministic_ai_kernel::scheduler::schedule(db, task_id),
            );
            return;
        }
        Some("fail-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            let reason = args.get(5).map(|s| s.as_str()).unwrap_or("worker_error");
            cli_expect(
                "fail-step",
                deterministic_ai_kernel::worker::fail_step(db, task_id, worker_id, step_id, reason),
            );
            return;
        }
        Some("start-step") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            cli_expect(
                "start-step",
                deterministic_ai_kernel::worker::start_step(db, task_id, worker_id, step_id),
            );
            return;
        }
        Some("heartbeat") => {
            let task_id = args.get(2).map(|s| s.as_str()).unwrap_or("task1");
            let worker_id = args.get(3).map(|s| s.as_str()).unwrap_or("worker-1");
            let step_id = args.get(4).map(|s| s.as_str()).expect("step id required");
            cli_expect(
                "heartbeat",
                deterministic_ai_kernel::worker::heartbeat(db, task_id, worker_id, step_id),
            );
            return;
        }
        Some("rmdb") => {
            let _ = fs::remove_file(db);
            println!("RMDB OK");
            return;
        }
        Some("models-list") => {
            deterministic_ai_kernel::lm_control::print_all_models_v0()
                .unwrap_or_else(|e| eprintln!("models-list: {e}"));
            return;
        }
        Some("models-loaded") => {
            deterministic_ai_kernel::lm_control::print_loaded_models()
                .unwrap_or_else(|e| eprintln!("models-loaded: {e}"));
            return;
        }
        Some("model-load") => {
            let id = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() {
                eprintln!("usage: model-load <model-id>");
                return;
            }
            deterministic_ai_kernel::lm_control::load_model(id)
                .unwrap_or_else(|e| eprintln!("model-load: {e}"));
            return;
        }
        Some("model-unload") => {
            let id = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() {
                eprintln!("usage: model-unload <model-id>");
                return;
            }
            deterministic_ai_kernel::lm_control::unload_model(id)
                .unwrap_or_else(|e| eprintln!("model-unload: {e}"));
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
            deterministic_ai_kernel::lm_control::smart_switch(id, gb)
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
                    .expect("failed to get duration")
                    .as_nanos()
            ))
            .to_string_lossy()
            .to_string();

        let report = deterministic_ai_kernel::cli_json::command_report(
            "integrity-json",
            integrity_json_report(&db),
        );

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
