use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::process::Command;

use crate::model_manifest;

pub mod policy;

use policy::switch_plan;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ModelInfo {
    id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct DoctorReport {
    pub free_gb: f64,
    pub mlx_models: usize,
    pub roles: Vec<DoctorRoleReport>,
    // RuntimeManager fields
    pub runtime_provider: String,
    pub runtime_pid: Option<u32>,
    pub runtime_host: String,
    pub runtime_port: u16,
    pub runtime_status: String,
    pub loaded_runtime_model: Option<String>,
    pub config_default_model: String,
    pub embedding_provider: Option<String>,
    pub embedding_model: Option<String>,
    pub embedding_endpoint: Option<String>,
    pub embedding_status: String,
    pub embedding_reason: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct DoctorRoleReport {
    pub role: String,
    pub manifest_model: String,
    pub env_model: String,
    pub in_sync: bool,
    /// Local model files exist on disk
    pub model_present: bool,
    /// MLX runtime responded to health probe
    pub runtime_ready: bool,
    /// manifest model_id matches what runtime reports
    pub model_id_match: bool,
    /// Composite: runtime_ready && model_id_match (replaces old model_available)
    pub model_available: bool,
    pub switch_ready: bool,
    pub threshold_gb: f64,
}

pub fn memory_snapshot() -> Result<String> {
    let output = Command::new("sh")
        .arg("-lc")
        .arg("vm_stat | head -n 20")
        .output()?;

    if !output.status.success() {
        return Err(anyhow!("vm_stat failed"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn free_memory_gb_estimate() -> Result<f64> {
    if let Ok(val) = std::env::var("DAK_FREE_GB_OVERRIDE") {
        return Ok(val.trim().parse::<f64>()?);
    }
    let output = Command::new("sh")
        .arg("-lc")
        .arg(
            r#"vm_stat | awk '
            /free/ {free=$3}
            /inactive/ {inactive=$3}
            /speculative/ {spec=$3}
            END {
                gsub("\\.", "", free); gsub("\\.", "", inactive); gsub("\\.", "", spec);
                pagesize=16384;
                total=(free+inactive+spec)*pagesize;
                printf "%.2f", total/1024/1024/1024;
            }'"#,
        )
        .output()?;

    if !output.status.success() {
        return Err(anyhow!("failed to estimate memory"));
    }

    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(s.parse::<f64>()?)
}

/// Probe the MLX runtime via native socket ping.
/// Returns (runtime_ready, model_ids).
pub fn probe_mlx_runtime() -> (bool, Vec<String>) {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        let ids = model_manifest::load_manifest()
            .map(|m| {
                m.models
                    .into_iter()
                    .filter(|r| r.enabled)
                    .map(|r| r.id)
                    .collect()
            })
            .unwrap_or_default();
        return (true, ids);
    }

    if let Ok(mgr) = crate::runtime_manager::RuntimeManager::load() {
        if let Ok(info) = mgr.status() {
            let ready = info.status == crate::runtime_manager::RuntimeStatus::Running;
            let ids = info.loaded_model.map(|m| vec![m]).unwrap_or_default();
            return (ready, ids);
        }
    }

    (false, vec![])
}

/// Returns model ids from MLX runtime (empty if unavailable).
pub fn list_models() -> Result<Vec<String>> {
    let (_, ids) = probe_mlx_runtime();
    Ok(ids)
}

/// Returns true if local model files exist on disk.
pub fn model_path_present() -> bool {
    std::env::var("OPENAI_MODEL")
        .map(|p| std::path::Path::new(&p).exists())
        .unwrap_or(false)
}

pub fn print_memory(threshold_gb: Option<f64>) -> Result<()> {
    let free = free_memory_gb_estimate()?;
    let threshold = threshold_gb.unwrap_or(6.0);
    println!("FREE_GB={:.2}", free);
    println!("THRESHOLD_GB={:.2}", threshold);
    println!("OK_TO_SWITCH={}", free >= threshold);
    println!("\n{}", memory_snapshot()?);
    Ok(())
}

pub fn safe_switch(role: &str) -> Result<()> {
    let free = free_memory_gb_estimate()?;
    let plan = switch_plan(role, free)?;
    let model = plan.model;
    let ram_class = plan.ram_class;
    let threshold = plan.threshold_gb;
    let free = plan.free_gb;

    if free < threshold {
        return Err(anyhow!(
            "not enough free memory for role {:?} model {:?}: {:.2} GB available, {:.2} GB required (ram_class={})",
            role,
            model,
            free,
            threshold,
            ram_class
        ));
    }

    let models = list_models()?;
    if !models.iter().any(|m| m == &model) {
        return Err(anyhow!("target model not available locally: {}", model));
    }

    let synced_model = model_manifest::sync_env_for_role(role)?;
    println!(
        "SAFE_SWITCH_OK role={} model={} ram_class={} threshold_gb={:.2} free_gb={:.2}",
        role, synced_model, ram_class, threshold, free
    );
    Ok(())
}

pub fn dry_run_switch(role: &str) -> Result<()> {
    let free = free_memory_gb_estimate()?;
    let plan = switch_plan(role, free)?;
    let model = plan.model;
    let ram_class = plan.ram_class;
    let threshold = plan.threshold_gb;
    let free = plan.free_gb;
    let models = list_models()?;
    let available = models.iter().any(|m| m == &model);
    let would_write_env = format!(
        "OPENAI_MODEL_{}={}",
        match role {
            "coding_assistant" => "CODING_ASSISTANT",
            "task_planning" => "TASK_PLANNING",
            "embeddings" => "EMBEDDINGS",
            _ => return Err(anyhow!("unsupported role {:?}", role)),
        },
        model
    );

    println!("DRY_RUN_ROLE={}", role);
    println!("DRY_RUN_MODEL={}", model);
    println!("DRY_RUN_RAM_CLASS={}", ram_class);
    println!("DRY_RUN_THRESHOLD_GB={:.2}", threshold);
    println!("DRY_RUN_FREE_GB={:.2}", free);
    println!("DRY_RUN_MODEL_AVAILABLE={}", available);
    println!("DRY_RUN_WOULD_WRITE={}", would_write_env);
    println!("DRY_RUN_OK_TO_SWITCH={}", free >= threshold && available);
    Ok(())
}

pub fn doctor() -> Result<DoctorReport> {
    let free_gb = free_memory_gb_estimate()?;
    let (runtime_ready, runtime_ids) = probe_mlx_runtime();
    let present = model_path_present();
    let rows = model_manifest::current_model_statuses()?;

    // Gather RuntimeManager info (graceful fallback if config missing).
    let (rt_provider, rt_pid, rt_host, rt_port, rt_status, rt_loaded, rt_config_model) =
        match crate::runtime_manager::RuntimeManager::load() {
            Ok(mgr) => {
                let info = mgr
                    .status()
                    .unwrap_or_else(|_| crate::runtime_manager::RuntimeInfo {
                        status: crate::runtime_manager::RuntimeStatus::Error,
                        pid: None,
                        host: mgr.config().host.clone(),
                        port: mgr.config().port,
                        provider: mgr.config().provider.clone(),
                        loaded_model: None,
                        config_model: mgr.config().default_model.clone(),
                        base_url: mgr.config().base_url(),
                    });
                (
                    info.provider,
                    info.pid,
                    info.host,
                    info.port,
                    info.status.to_string(),
                    info.loaded_model,
                    info.config_model,
                )
            }
            Err(_) => (
                "mlx".to_string(),
                None,
                "127.0.0.1".to_string(),
                8080,
                "unknown".to_string(),
                None,
                std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "<not set>".to_string()),
            ),
        };

    let (emb_provider, emb_model, emb_endpoint, emb_status, emb_reason) =
        match crate::runtime_manager::EmbeddingRuntimeManager::load() {
            Ok(mgr) => match mgr.status() {
                Ok(info) => {
                    let status_str = match info.status {
                        crate::runtime_manager::RuntimeStatus::Running => "AVAILABLE".to_string(),
                        crate::runtime_manager::RuntimeStatus::Starting => "STARTING".to_string(),
                        crate::runtime_manager::RuntimeStatus::Stopped => "BLOCKED".to_string(),
                        crate::runtime_manager::RuntimeStatus::Error => "BLOCKED".to_string(),
                    };
                    (
                        Some(info.provider.clone()),
                        Some(info.config_model.clone()),
                        Some(info.base_url.clone()),
                        status_str,
                        if info.status != crate::runtime_manager::RuntimeStatus::Running {
                            Some("local embedding backend unavailable".to_string())
                        } else {
                            None
                        },
                    )
                }
                Err(e) => (
                    None,
                    None,
                    None,
                    "NOT_CONFIGURED".to_string(),
                    Some(e.to_string()),
                ),
            },
            Err(_) => (None, None, None, "NOT_CONFIGURED".to_string(), None),
        };

    let mut roles = Vec::new();
    for row in rows {
        let threshold = model_manifest::threshold_gb_for_ram_class(
            &model_manifest::best_enabled_model_for_role(&row.role)?.ram_class,
        )?;
        // MLX runtime returns the local path as model id (e.g. /Users/.../Models/foo).
        // Match if: (1) exact, (2) runtime id == OPENAI_MODEL env path,
        // (3) basename of runtime id matches basename of OPENAI_MODEL.
        let _local_model_path = std::env::var("OPENAI_MODEL").unwrap_or_default();
        let model_id_match_mlx = runtime_ids.iter().any(|runtime_id| {
            if runtime_id == &row.manifest_model {
                return true;
            }
            if !_local_model_path.is_empty() && runtime_id == &_local_model_path {
                return true;
            }
            let rt_base = std::path::Path::new(runtime_id.as_str())
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let lp_base = std::path::Path::new(_local_model_path.as_str())
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            !rt_base.is_empty() && !lp_base.is_empty() && rt_base == lp_base
        });

        let is_embeddings = row.role == "embeddings";
        let (model_available, model_id_match, role_runtime_ready) = if is_embeddings {
            let matched = emb_model.as_ref() == Some(&row.manifest_model);
            (
                emb_status == "AVAILABLE",
                matched,
                emb_status == "AVAILABLE",
            )
        } else {
            let available = runtime_ready && model_id_match_mlx;
            (available, model_id_match_mlx, runtime_ready)
        };

        let switch_ready = if is_embeddings {
            model_available
        } else {
            free_gb >= threshold && model_available
        };

        roles.push(DoctorRoleReport {
            role: row.role,
            manifest_model: row.manifest_model,
            env_model: row.env_model.unwrap_or_else(|| "<missing>".to_string()),
            in_sync: row.in_sync,
            model_present: present,
            runtime_ready: role_runtime_ready,
            model_id_match,
            model_available,
            switch_ready,
            threshold_gb: threshold,
        });
    }

    Ok(DoctorReport {
        free_gb,
        mlx_models: runtime_ids.len(),
        roles,
        runtime_provider: rt_provider,
        runtime_pid: rt_pid,
        runtime_host: rt_host,
        runtime_port: rt_port,
        runtime_status: rt_status,
        loaded_runtime_model: rt_loaded,
        config_default_model: rt_config_model,
        embedding_provider: emb_provider,
        embedding_model: emb_model,
        embedding_endpoint: emb_endpoint,
        embedding_status: emb_status,
        embedding_reason: emb_reason,
    })
}

pub fn auto_route(role: &str) -> Result<()> {
    let report = doctor()?;
    let row = report
        .roles
        .iter()
        .find(|r| r.role == role)
        .ok_or_else(|| anyhow!("role not found in doctor report: {}", role))?;

    if !row.model_available {
        return Err(anyhow!(
            "auto-route blocked: model {:?} for role {:?} is not available locally",
            row.manifest_model,
            role
        ));
    }

    // Skip RAM check if model already loaded in MLX runtime
    let already_loaded = probe_mlx_runtime().1.contains(&row.manifest_model);

    if !row.switch_ready && !already_loaded {
        return Err(anyhow!(
            "auto-route blocked: role {:?} is not ready, free_gb={:.2}, threshold_gb={:.2}",
            role,
            report.free_gb,
            row.threshold_gb
        ));
    }

    let synced_model = model_manifest::sync_env_for_role(role)?;
    println!(
        "AUTO_ROUTE_OK role={} model={} free_gb={:.2} threshold_gb={:.2}",
        role, synced_model, report.free_gb, row.threshold_gb
    );
    Ok(())
}

/// Returns the doctor report as a JSON Value.
/// Formatting (CLI envelope) must be done by the interface layer (main.rs), not here.
pub fn doctor_json_report() -> Result<serde_json::Value> {
    let report = doctor()?;
    Ok(serde_json::to_value(report)?)
}

fn total_memory_gb() -> Result<f64> {
    let output = Command::new("sysctl").args(["-n", "hw.memsize"]).output()?;
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let bytes = s.parse::<u64>()?;
    Ok(bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

fn cpu_brand_string() -> String {
    Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .unwrap_or_else(|_| "Apple Silicon".to_string())
}

pub async fn print_doctor_text() -> Result<()> {
    let report = doctor()?;

    let status_icon = match report.runtime_status.as_str() {
        "running" => "●",
        "stopped" => "○",
        "starting" => "◐",
        _ => "✗",
    };

    // Smoke tests (only if backend is not mock or if we are verifying layout)
    let start_time = std::time::Instant::now();
    let llm_smoke_res = crate::llm::coding_assistant("Reply with exactly one word: OK").await;
    let inference_latency = start_time.elapsed();

    let (llm_smoke_status, llm_ok) = match &llm_smoke_res {
        Ok(text) if text.trim().to_lowercase().contains("ok") => ("OK".to_string(), true),
        Ok(text) => (
            format!("FAILED (unexpected response: '{}')", text.trim()),
            false,
        ),
        Err(e) => (format!("FAILED ({})", e), false),
    };

    let planner_smoke_res =
        crate::llm::task_planner("Reply with exactly: Task planner from Rust works").await;
    let (planner_smoke_status, planner_ok) = match &planner_smoke_res {
        Ok(text) if text.trim().contains("Task planner from Rust works") => {
            ("OK".to_string(), true)
        }
        Ok(text) => (
            format!("FAILED (unexpected response: '{}')", text.trim()),
            false,
        ),
        Err(e) => (format!("FAILED ({})", e), false),
    };

    let embeddings_smoke_res =
        crate::embeddings::embed_text("deterministic kernel embeddings smoke test").await;
    let (embeddings_smoke_status, embed_ok) = match &embeddings_smoke_res {
        Ok(vec) if !vec.is_empty() => (format!("OK (Dim: {})", vec.len()), true),
        Ok(_) => ("FAILED (empty vector)".to_string(), false),
        Err(e) => (format!("FAILED ({})", e), false),
    };

    // System resources
    let total_gb = total_memory_gb().unwrap_or(16.0);
    let cpu_brand = cpu_brand_string();
    let metal_mem = if cpu_brand.contains("Apple") {
        format!("Unified ({:.1} GB Shared)", total_gb)
    } else {
        format!("{:.1} GB System RAM", total_gb)
    };

    let context_window = if report
        .loaded_runtime_model
        .as_deref()
        .unwrap_or("")
        .contains("gemma4")
        || report.config_default_model.contains("gemma4")
        || report
            .loaded_runtime_model
            .as_deref()
            .unwrap_or("")
            .contains("gemma-4")
        || report.config_default_model.contains("gemma-4")
    {
        "131,072 tokens"
    } else {
        "8,192 tokens (default)"
    };

    // Storage & Database status
    let db_path = std::env::var("KERNEL_DB_PATH").unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|d| d.join("kernel.db").to_string_lossy().to_string())
            .unwrap_or_else(|_| "kernel.db".to_string())
    });

    let mut db_status_str = "OK".to_string();
    let mut event_count = 0;
    let mut causal_units = 0;
    let mut snapshots_count = 0;
    let mut active_steps = 0;

    let db_conn = rusqlite::Connection::open(&db_path);
    match &db_conn {
        Ok(conn) => {
            event_count = conn
                .query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get::<_, i64>(0))
                .unwrap_or(0);
            causal_units = conn
                .query_row(
                    "SELECT COUNT(DISTINCT causal_unit_id) FROM event_log",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
            snapshots_count = conn
                .query_row("SELECT COUNT(*) FROM state_snapshots", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap_or(0);
            active_steps = conn
                .query_row(
                    "SELECT COUNT(*) FROM step_status WHERE outcome = 'Running'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0);
        }
        Err(e) => {
            db_status_str = format!("ERROR ({})", e);
        }
    }

    let everything_ok = if llm_ok && planner_ok && embed_ok && db_conn.is_ok() {
        "YES"
    } else {
        "NO"
    };

    println!("═══════════════════════════════════════════");
    println!("  Deterministic AI Kernel — Doctor Report");
    println!("═══════════════════════════════════════════");
    println!("Runtime:               {}", report.runtime_provider);
    println!("Provider:              {}", report.runtime_provider);
    println!(
        "PID:                   {}",
        report
            .runtime_pid
            .map_or("—".to_string(), |p| p.to_string())
    );
    println!("Host:                  {}", report.runtime_host);
    println!("Port:                  {}", report.runtime_port);
    println!(
        "Runtime Status:        {status_icon} {}",
        report.runtime_status
    );
    println!(
        "Loaded Runtime Model:  {}",
        report.loaded_runtime_model.as_deref().unwrap_or("—")
    );
    println!("Manifest Default Model: {}", report.config_default_model);
    println!();
    println!("Embedding Runtime:");
    if report.embedding_status == "NOT_CONFIGURED" {
        println!("  Status:              NOT_CONFIGURED");
    } else {
        println!(
            "  Provider:            {}",
            report.embedding_provider.as_deref().unwrap_or("—")
        );
        println!(
            "  Model:               {}",
            report.embedding_model.as_deref().unwrap_or("—")
        );
        println!(
            "  Endpoint:            {}",
            report.embedding_endpoint.as_deref().unwrap_or("—")
        );
        let emb_icon = match report.embedding_status.as_str() {
            "AVAILABLE" => "●",
            _ => "✗",
        };
        println!(
            "  Status:              {} {}",
            emb_icon, report.embedding_status
        );
        if let Some(reason) = &report.embedding_reason {
            println!("  Reason:              {}", reason);
        }
    }
    println!();
    println!("Model Roles:");
    for row in &report.roles {
        let sync_icon = if row.in_sync { "✓" } else { "✗" };
        println!(
            "  {sync_icon} ROLE={:<20} MODEL={}",
            row.role, row.manifest_model
        );
    }
    println!();
    println!("Smoke Tests:");
    println!("  LLM Smoke:           {}", llm_smoke_status);
    println!("  Planner Smoke:       {}", planner_smoke_status);
    println!("  Embeddings Smoke:    {}", embeddings_smoke_status);
    println!("  Inference Latency:   {:.2?}", inference_latency);
    println!();
    println!("System Resources:");
    let used_gb = total_gb - report.free_gb;
    let used_pct = (used_gb / total_gb) * 100.0;
    println!(
        "  RAM Usage:           {:.2} GB used / {:.2} GB total ({:.1}%)",
        used_gb, total_gb, used_pct
    );
    println!("  Metal Memory:        {}", metal_mem);
    println!("  Context Window:      {}", context_window);
    println!();
    println!("Storage & Execution:");
    println!(
        "  Database Status:     {} (Path: {})",
        db_status_str, db_path
    );
    println!(
        "  Replay Status:       Active ({} events, {} causal units, {} snapshots)",
        event_count, causal_units, snapshots_count
    );
    let worker_str = if active_steps > 0 {
        format!("Active ({} running steps)", active_steps)
    } else {
        "Idle".to_string()
    };
    println!("  Worker Status:       {}", worker_str);
    println!();
    println!("Everything OK:         {}", everything_ok);
    println!("═══════════════════════════════════════════");
    Ok(())
}

// ── MLX-only: load/unload managed by mlx_lm.server ──────────────────────────
// LM Studio v0 API removed. Use probe_mlx_runtime() or list_models().

#[allow(dead_code)]
pub fn list_all_models_v0() -> anyhow::Result<Vec<String>> {
    Err(anyhow::anyhow!(
        "list_all_models_v0: not available — use probe_mlx_runtime()"
    ))
}

/// MLX runtime manages model lifecycle via mlx_lm.server.
#[allow(dead_code)]
pub fn load_model(_identifier: &str) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "load_model: not supported in MLX architecture"
    ))
}

#[allow(dead_code)]
pub fn unload_model(_identifier: &str) -> anyhow::Result<()> {
    Err(anyhow::anyhow!(
        "unload_model: not supported in MLX architecture"
    ))
}

// ── smart_switch ──────────────────────────────────────────────────────────────

pub fn smart_switch(target_model: &str, required_gb: f64) -> Result<()> {
    let free_gb = free_memory_gb_estimate()?;
    println!(
        "SMART_SWITCH target={target_model} required_gb={required_gb:.2} free_gb={free_gb:.2}"
    );

    let (runtime_ready, loaded) = probe_mlx_runtime();
    println!("MLX_RUNTIME_READY={runtime_ready} LOADED_NOW {:?}", loaded);

    if loaded.contains(&target_model.to_string()) {
        println!("ALREADY_LOADED model={target_model}");
        return Ok(());
    }

    if free_gb < required_gb {
        return Err(anyhow!(
            "smart_switch: not enough memory: {free_gb:.2} GB free, {required_gb:.2} GB required. \
             Restart mlx_lm.server with the target model."
        ));
    }

    Err(anyhow!(
        "smart_switch: model '{target_model}' not loaded in MLX runtime. \
         Start: mlx_lm.server --model {target_model}"
    ))
}

// ── CLI print helpers ─────────────────────────────────────────────────────────

pub fn print_loaded_models() -> Result<()> {
    let (ready, loaded) = probe_mlx_runtime();
    if !ready {
        println!("MLX_RUNTIME_OFFLINE");
        return Ok(());
    }
    if loaded.is_empty() {
        println!("NO_MODELS_LOADED");
    } else {
        for m in &loaded {
            println!("LOADED model={m}");
        }
    }
    Ok(())
}

pub fn print_all_models_v0() -> Result<()> {
    let all = list_models()?;
    let free_gb = free_memory_gb_estimate()?;
    println!("FREE_GB={free_gb:.2}  TOTAL_MODELS={}", all.len());
    println!();
    for id in &all {
        println!("  [mlx_runtime ] {id}");
    }
    Ok(())
}

pub async fn send_prompt(role: &str, user_text: &str) -> Result<String> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        return Ok(format!("[mock] role={role} input={user_text}"));
    }

    let system_prompt = model_manifest::system_prompt_for_role(role)?;
    crate::llm::chat_with_role(role, &system_prompt, user_text).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gb_helper_works() {
        assert!(free_memory_gb_estimate().is_ok());
    }

    #[test]
    fn doctor_json_report_has_expected_shape() {
        // doctor_json_report() must return raw Value — no CLI envelope here.
        // The interface layer (main.rs / cli_json) is responsible for wrapping.
        let raw = doctor_json_report().expect("failed to generate doctor json report");
        assert!(raw.get("free_gb").is_some());
        assert!(raw.get("mlx_models").is_some());
        assert!(raw.get("roles").is_some());
        assert!(raw["roles"].is_array());

        if let Some(first) = raw["roles"].as_array().and_then(|rows| rows.first()) {
            assert!(first.get("role").is_some());
            assert!(first.get("manifest_model").is_some());
            assert!(first.get("env_model").is_some());
            assert!(first.get("in_sync").is_some());
            assert!(first.get("model_available").is_some());
            assert!(first.get("switch_ready").is_some());
            assert!(first.get("threshold_gb").is_some());
        }
    }
}
