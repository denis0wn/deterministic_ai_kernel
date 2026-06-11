mod cli_json;
mod effects;
mod embeddings;
mod event_bus;
mod execution;
mod leases;
mod llm;
mod kernel_types;
mod lm_control;
mod model_manifest;
mod model_registry;
mod replay;
mod scheduler;
mod snapshot;
mod worker;
mod workflow;

use cli_json::{capsule_summary_report, comparison_report, print_json_report};
use effects::execute_effects;
use execution::runtime::Runtime;
use leases::{expire_leases, seed_demo_leases};
use replay::engine::replay_validate;
use replay::capsule::build_replay_capsule;
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
    conn.execute_batch(include_str!("../event_bus/schema.sql")).unwrap();
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
    let report = integrity_json_report(db);
    let envelope = cli_json::command_report("integrity-json", report);
    cli_json::print_json_report(&envelope);
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

            let bus = event_bus::EventBus::new(db).unwrap();
            match build_replay_capsule(&bus, &task_id) {
                Ok(capsule) => {
                    if let Err(err) = capsule.validate() {
                        eprintln!("capture-capsule-save invalid capsule: {}", err);
                        std::process::exit(1);
                    }

                    let event_count = capsule.event_ids.len();
                    let node_count = capsule.state_graph.nodes.len();
                    let edge_count = capsule.state_graph.edges.len();
                    bus.save_replay_capsule(&capsule).unwrap();

                    if json_output {
                        let report =
                            capsule_summary_report("capture-capsule-save", &task_id, &capsule, true);
                        let envelope = cli_json::command_report("capture-capsule-save", report);
                        print_json_report(&envelope);
                    } else {
                        println!(
                            "CAPTURE_CAPSULE_SAVE_OK\t{}\t{}\tevents={}\tnodes={}\tedges={}",
                            capsule.execution_id,
                            capsule.capsule_id,
                            event_count,
                            node_count,
                            edge_count
                        );
                    }
                }
                Err(e) => {
                    eprintln!("capture-capsule-save failed: {e}");
                    std::process::exit(1);
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

            let bus = event_bus::EventBus::new(db).unwrap();
            let left_capsule = match bus.latest_replay_capsule(&left) {
                Ok(Some(c)) => c,
                Ok(None) => {
                    eprintln!("no replay capsule found for task_id={}", left);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("compare-capsules failed: {e}");
                    std::process::exit(1);
                }
            };
            let right_capsule = match bus.latest_replay_capsule(&right) {
                Ok(Some(c)) => c,
                Ok(None) => {
                    eprintln!("no replay capsule found for task_id={}", right);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("compare-capsules failed: {e}");
                    std::process::exit(1);
                }
            };

            let left_valid = left_capsule.validate();
            let right_valid = right_capsule.validate();

            let status = if left_valid.is_err() || right_valid.is_err() {
                "structurally_invalid"
            } else if left_capsule.event_ids == right_capsule.event_ids
                && left_capsule.state_graph.nodes == right_capsule.state_graph.nodes
                && left_capsule.state_graph.edges == right_capsule.state_graph.edges
            {
                "identical"
            } else {
                "divergent"
            };

            let explanation = match status {
                "structurally_invalid" => {
                    let mut reasons = Vec::new();
                    if let Err(err) = &left_valid {
                        reasons.push(format!("left invalid: {}", err));
                    }
                    if let Err(err) = &right_valid {
                        reasons.push(format!("right invalid: {}", err));
                    }
                    reasons.join("; ")
                }
                "identical" => "event_ids, nodes, and edges match".to_string(),
                "divergent" => {
                    let mut reasons = Vec::new();
                    if left_capsule.event_ids != right_capsule.event_ids {
                        reasons.push(format!(
                            "event_ids differ (left={}, right={})",
                            left_capsule.event_ids.len(),
                            right_capsule.event_ids.len()
                        ));
                    }
                    if left_capsule.state_graph.nodes != right_capsule.state_graph.nodes {
                        reasons.push(format!(
                            "nodes differ (left={}, right={})",
                            left_capsule.state_graph.nodes.len(),
                            right_capsule.state_graph.nodes.len()
                        ));
                    }
                    if left_capsule.state_graph.edges != right_capsule.state_graph.edges {
                        reasons.push(format!(
                            "edges differ (left={}, right={})",
                            left_capsule.state_graph.edges.len(),
                            right_capsule.state_graph.edges.len()
                        ));
                    }
                    if reasons.is_empty() {
                        "capsules differ".to_string()
                    } else {
                        reasons.join("; ")
                    }
                }
                _ => "unknown comparison state".to_string(),
            };

            let left_only_events: Vec<_> = left_capsule
                .event_ids
                .iter()
                .filter(|id| !right_capsule.event_ids.contains(id))
                .cloned()
                .collect();
            let right_only_events: Vec<_> = right_capsule
                .event_ids
                .iter()
                .filter(|id| !left_capsule.event_ids.contains(id))
                .cloned()
                .collect();

            let left_node_pairs: Vec<(String, serde_json::Value)> = left_capsule
                .state_graph
                .nodes
                .iter()
                .map(|n| {
                    let v = serde_json::to_value(n).unwrap();
                    let key = serde_json::to_string(&v).unwrap();
                    (key, v)
                })
                .collect();
            let right_node_pairs: Vec<(String, serde_json::Value)> = right_capsule
                .state_graph
                .nodes
                .iter()
                .map(|n| {
                    let v = serde_json::to_value(n).unwrap();
                    let key = serde_json::to_string(&v).unwrap();
                    (key, v)
                })
                .collect();

            let left_node_keys: std::collections::BTreeSet<_> =
                left_node_pairs.iter().map(|(k, _)| k.clone()).collect();
            let right_node_keys: std::collections::BTreeSet<_> =
                right_node_pairs.iter().map(|(k, _)| k.clone()).collect();

            let left_only_nodes: Vec<_> = left_node_pairs
                .iter()
                .filter(|(k, _)| !right_node_keys.contains(k))
                .map(|(_, v)| v.clone())
                .collect();
            let right_only_nodes: Vec<_> = right_node_pairs
                .iter()
                .filter(|(k, _)| !left_node_keys.contains(k))
                .map(|(_, v)| v.clone())
                .collect();

            let left_edge_pairs: Vec<(String, serde_json::Value)> = left_capsule
                .state_graph
                .edges
                .iter()
                .map(|e| {
                    let v = serde_json::to_value(e).unwrap();
                    let key = serde_json::to_string(&v).unwrap();
                    (key, v)
                })
                .collect();
            let right_edge_pairs: Vec<(String, serde_json::Value)> = right_capsule
                .state_graph
                .edges
                .iter()
                .map(|e| {
                    let v = serde_json::to_value(e).unwrap();
                    let key = serde_json::to_string(&v).unwrap();
                    (key, v)
                })
                .collect();

            let left_edge_keys: std::collections::BTreeSet<_> =
                left_edge_pairs.iter().map(|(k, _)| k.clone()).collect();
            let right_edge_keys: std::collections::BTreeSet<_> =
                right_edge_pairs.iter().map(|(k, _)| k.clone()).collect();

            let left_only_edges: Vec<_> = left_edge_pairs
                .iter()
                .filter(|(k, _)| !right_edge_keys.contains(k))
                .map(|(_, v)| v.clone())
                .collect();
            let right_only_edges: Vec<_> = right_edge_pairs
                .iter()
                .filter(|(k, _)| !left_edge_keys.contains(k))
                .map(|(_, v)| v.clone())
                .collect();

            if json_output {
                let report = comparison_report(cli_json::ComparisonReportInput {
                    left_task_id: &left,
                    right_task_id: &right,
                    left_capsule: &left_capsule,
                    right_capsule: &right_capsule,
                    left_valid: left_valid.is_ok(),
                    right_valid: right_valid.is_ok(),
                    status,
                    explanation: &explanation,
                    left_only_events,
                    right_only_events,
                    left_only_nodes,
                    right_only_nodes,
                    left_only_edges,
                    right_only_edges,
                });
                let envelope = cli_json::command_report("compare-capsules", report);
                print_json_report(&envelope);
            } else if explain {
                println!(
                    "COMPARE_CAPSULES_OK\t{}\t{}\tstatus={}\texplanation={}",
                    left_capsule.capsule_id,
                    right_capsule.capsule_id,
                    status,
                    explanation
                );
            } else {
                println!(
                    "COMPARE_CAPSULES_OK\t{}\t{}\tstatus={}",
                    left_capsule.capsule_id,
                    right_capsule.capsule_id,
                    status
                );
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

            let bus = event_bus::EventBus::new(db).unwrap();
            match bus.latest_replay_capsule(&task_id) {
                Ok(Some(capsule)) => {
                    let valid = capsule.validate().is_ok();

                    if json_output {
                        let report =
                            capsule_summary_report("replay-capsule", &task_id, &capsule, valid);
                        let envelope = cli_json::command_report("replay-capsule", report);
                        print_json_report(&envelope);
                    } else {
                        println!(
                            "REPLAY_CAPSULE_OK\t{}\t{}\tevents={}\tnodes={}\tedges={}\tvalid={}",
                            capsule.execution_id,
                            capsule.capsule_id,
                            capsule.event_ids.len(),
                            capsule.state_graph.nodes.len(),
                            capsule.state_graph.edges.len(),
                            valid
                        );
                    }
                }
                Ok(None) => {
                    eprintln!("no replay capsule found for task_id={}", task_id);
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("replay-capsule failed: {e}");
                    std::process::exit(1);
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
            match bus.append_semantic_artifact(&task_id, &step_id, 0, "semantic_bias_v1", &payload) {
                Ok(()) => {
                    println!("EMIT_BIAS_ARTIFACT_OK\t{}\t{}", task_id, step_id);
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
                    if let Some(row) = rows.into_iter().find(|r| r.artifact_type == "semantic_bias_v1") {
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
                            "{}\t{}\t{}\t{}\t{}\t{}",
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
        assert_eq!(report.get("schema_version").and_then(|v| v.as_str()), Some("cli-json-v1"));
        assert_eq!(report.get("command").and_then(|v| v.as_str()), Some("integrity-json"));
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

