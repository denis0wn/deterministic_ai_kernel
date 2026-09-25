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
    // No shell: argv spawn + Rust-side truncation (was `sh -lc "vm_stat | head"`).
    let output = Command::new("vm_stat").output()?;

    if !output.status.success() {
        return Err(anyhow!("vm_stat failed"));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().take(20).collect::<Vec<_>>().join("\n"))
}

/// Rust port of the former awk pipeline: free+inactive+speculative pages
/// times 16384-byte page size, in GiB. vm_stat prints "Pages free:  12345."
/// — field 3 carries a trailing dot.
fn vm_stat_free_gb(text: &str) -> Result<f64> {
    let mut pages: u64 = 0;
    let mut seen = 0u32;
    for line in text.lines() {
        let lower = line.to_lowercase();
        let matched = (lower.contains("free") && !lower.contains("fault"))
            || lower.contains("inactive")
            || lower.contains("speculative");
        if !matched {
            continue;
        }
        let num = lower
            .split_whitespace()
            .nth(2)
            .map(|t| t.trim_end_matches('.').to_string())
            .and_then(|t| t.parse::<u64>().ok())
            .ok_or_else(|| anyhow!("vm_stat: unparseable line: {line}"))?;
        pages += num;
        seen += 1;
    }
    if seen == 0 {
        return Err(anyhow!("vm_stat: no free/inactive/speculative lines"));
    }
    Ok(pages as f64 * 16384.0 / 1024.0 / 1024.0 / 1024.0)
}

pub fn free_memory_gb_estimate() -> Result<f64> {
    if let Ok(val) = std::env::var("DAK_FREE_GB_OVERRIDE") {
        return Ok(val.trim().parse::<f64>()?);
    }
    let output = Command::new("vm_stat").output()?;

    if !output.status.success() {
        return Err(anyhow!("failed to estimate memory"));
    }

    let s = String::from_utf8_lossy(&output.stdout);
    vm_stat_free_gb(&s)
}

/// Probe the MLX runtime via OPENAI_BASE_URL/v1/models.
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
    let base =
        std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_string());
    let url = format!("{}/models", base.trim_end_matches('/'));
    let output = match Command::new("curl")
        .args(["-s", "--max-time", "3", &url])
        .output()
    {
        Ok(o) => o,
        Err(_) => return (false, vec![]),
    };
    if !output.status.success() || output.stdout.is_empty() {
        return (false, vec![]);
    }
    match serde_json::from_slice::<ModelsResponse>(&output.stdout) {
        Ok(parsed) => (true, parsed.data.into_iter().map(|m| m.id).collect()),
        Err(_) => (false, vec![]),
    }
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

    let mut roles = Vec::new();
    for row in rows {
        let threshold = model_manifest::threshold_gb_for_ram_class(
            &model_manifest::best_enabled_model_for_role(&row.role)?.ram_class,
        )?;
        // MLX runtime returns the local path as model id (e.g. /Users/.../Models/foo).
        // Match if: (1) exact, (2) runtime id == OPENAI_MODEL env path,
        // (3) basename of runtime id matches basename of OPENAI_MODEL.
        let _local_model_path = std::env::var("OPENAI_MODEL").unwrap_or_default();
        let model_id_match = runtime_ids.iter().any(|runtime_id| {
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
        // model_available = runtime is up AND runtime knows this model_id
        let model_available = runtime_ready && model_id_match;
        roles.push(DoctorRoleReport {
            role: row.role,
            manifest_model: row.manifest_model,
            env_model: row.env_model.unwrap_or_else(|| "<missing>".to_string()),
            in_sync: row.in_sync,
            model_present: present,
            runtime_ready,
            model_id_match,
            model_available,
            switch_ready: free_gb >= threshold && model_available,
            threshold_gb: threshold,
        });
    }

    Ok(DoctorReport {
        free_gb,
        mlx_models: runtime_ids.len(),
        roles,
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

pub fn print_doctor_text() -> Result<()> {
    let report = doctor()?;
    println!("FREE_GB={:.2}", report.free_gb);
    println!("MLX_MODELS={}", report.mlx_models);
    let r0 = report.roles.first();
    if let Some(r) = r0 {
        println!("MODEL_PRESENT={}", r.model_present);
        println!("RUNTIME_READY={}", r.runtime_ready);
        println!("MODEL_ID_MATCH={}", r.model_id_match);
    }
    println!();

    for row in report.roles {
        println!("ROLE={}", row.role);
        println!("MANIFEST_MODEL={}", row.manifest_model);
        println!("ENV_MODEL={}", row.env_model);
        println!("IN_SYNC={}", row.in_sync);
        println!("MODEL_AVAILABLE={}", row.model_available);
        println!("SWITCH_READY={}", row.switch_ready);
        println!("THRESHOLD_GB={:.2}", row.threshold_gb);
        println!();
    }

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
    #[cfg(target_os = "macos")]
    fn parse_gb_helper_works() {
        // Real vm_stat exists only on macOS; the pure parser is covered by
        // vm_stat_parser_* everywhere.
        assert!(free_memory_gb_estimate().is_ok());
    }

    #[test]
    fn vm_stat_parser_matches_awk_reference() {
        // Real macOS vm_stat fixture (M4 Pro, page size 16384). Reference
        // value computed by the former awk pipeline on this exact text:
        // (30970 + 600138 + 6266) * 16384 / 2^30 = 9.7255…
        let fixture = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
Pages free:                                    30970.\n\
Pages active:                                 600964.\n\
Pages inactive:                               600138.\n\
Pages speculative:                              6266.\n\
Pages wired down:                             200946.\n\
\"Translation faults\":                      273392692.\n";
        let gb = vm_stat_free_gb(fixture).unwrap();
        assert!((gb - 9.7255).abs() < 0.001, "got {gb}");
    }

    #[test]
    fn vm_stat_parser_fail_closed_on_garbage() {
        assert!(vm_stat_free_gb("no memory fields here").is_err());
    }

    #[test]
    fn doctor_json_report_has_expected_shape() {
        // doctor_json_report() must return raw Value — no CLI envelope here.
        // The interface layer (main.rs / cli_json) is responsible for wrapping.
        // Memory probe is mocked: vm_stat exists only on macOS.
        std::env::set_var("DAK_FREE_GB_OVERRIDE", "16.0");
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
