use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::process::Command;

use crate::llm;
use crate::model_manifest;

pub mod policy;

use policy::switch_plan;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: String,
}

#[derive(Debug, serde::Serialize)]
pub struct DoctorReport {
    pub free_gb: f64,
    pub lm_studio_models: usize,
    pub roles: Vec<DoctorRoleReport>,
}

#[derive(Debug, serde::Serialize)]
pub struct DoctorRoleReport {
    pub role: String,
    pub manifest_model: String,
    pub env_model: String,
    pub in_sync: bool,
    pub model_available: bool,
    pub model_id: String,
    pub backend: String,
    pub path: String,
    pub status: String,
    pub error: String,
    pub switch_ready: bool,
    pub threshold_gb: f64,
}

#[derive(Debug, serde::Deserialize, Clone)]
pub struct V0ModelInfo {
    pub id: String,
    pub state: Option<String>,
    pub arch: Option<String>,
    pub quantization: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct V0ModelsResponse {
    data: Vec<V0ModelInfo>,
}

const LM_STUDIO_BASE: &str = "http://127.0.0.1:1234";

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

#[allow(dead_code)]
pub fn list_models() -> Result<Vec<String>> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        let manifest = model_manifest::load_manifest()?;
        return Ok(manifest
            .models
            .into_iter()
            .filter(|m| m.enabled)
            .map(|m| m.id)
            .collect());
    }

    let output = Command::new("curl")
        .args(["-s", "http://127.0.0.1:1234/v1/models"])
        .output()?;

    if !output.status.success() {
        return Err(anyhow!("failed to query LM Studio models endpoint"));
    }

    let parsed: ModelsResponse = serde_json::from_slice(&output.stdout)?;
    Ok(parsed.data.into_iter().map(|m| m.id).collect())
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

    let verified = model_manifest::verify_mlx_model_by_id(&model)?;
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

    println!(
        "SAFE_SWITCH_OK role={} model={} ram_class={} threshold_gb={:.2} free_gb={:.2}",
        role, verified.id, ram_class, threshold, free
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
    let verified = model_manifest::verify_mlx_model_by_id(&model)?;
    let env_key = model_manifest::env_key_for_role(role)?;
    let model_available = true;

    println!("DRY_RUN_ROLE={}", role);
    println!("DRY_RUN_MODEL={}", verified.id);
    println!("DRY_RUN_RAM_CLASS={}", ram_class);
    println!("DRY_RUN_THRESHOLD_GB={:.2}", threshold);
    println!("DRY_RUN_FREE_GB={:.2}", free);
    println!("DRY_RUN_MODEL_AVAILABLE={}", model_available);
    println!("DRY_RUN_WOULD_WRITE={}={}", env_key, verified.id);
    println!("DRY_RUN_OK_TO_SWITCH={}", free >= threshold);
    Ok(())
}

pub fn doctor() -> Result<DoctorReport> {
    let free_gb = free_memory_gb_estimate()?;
    let rows = model_manifest::current_model_statuses()?;
    let loaded_models = list_loaded_models().unwrap_or_default();

    let mut roles = Vec::new();
    for row in rows {
        let threshold = if row.role == "task_planning" {
            7.50
        } else {
            model_manifest::threshold_gb_for_ram_class(
                &model_manifest::best_enabled_model_for_role(&row.role)?.ram_class,
            )?
        };

        let model_available = row.status == "ready";
        let already_loaded = loaded_models.iter().any(|m| m == &row.model_id);
        let switch_ready = model_available && (free_gb >= threshold || already_loaded);
        roles.push(DoctorRoleReport {
            role: row.role,
            manifest_model: row.manifest_model.clone(),
            env_model: row
                .env_model
                .clone()
                .unwrap_or_else(|| "<missing>".to_string()),
            in_sync: row.in_sync,
            model_available,
            model_id: row.model_id,
            backend: row.backend,
            path: row.path,
            status: row.status.clone(),
            error: row.error.clone(),
            switch_ready,
            threshold_gb: threshold,
        });
    }

    let lm_studio_models = if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        roles.len().max(1)
    } else {
        0
    };

    Ok(DoctorReport {
        free_gb,
        lm_studio_models,
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

    if row.status != "ready" {
        return Err(anyhow!(
            "auto-route blocked: role {:?} model_id {:?} is not ready: {}",
            role,
            row.model_id,
            row.error
        ));
    }

    if !row.switch_ready {
        return Err(anyhow!(
            "auto-route blocked: role {:?} is not ready, free_gb={:.2}, threshold_gb={:.2}",
            role,
            report.free_gb,
            row.threshold_gb
        ));
    }

    println!(
        "AUTO_ROUTE_OK role={} model_id={} backend={} path={} free_gb={:.2} threshold_gb={:.2}",
        role, row.model_id, row.backend, row.path, report.free_gb, row.threshold_gb
    );
    Ok(())
}

pub async fn send_prompt(role: &str, text: &str) -> Result<String> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        return Ok(format!("mock response for role={role}: {text}"));
    }

    let system_prompt = model_manifest::system_prompt_for_role(role)?;
    llm::chat_with_role(role, &system_prompt, text).await
}

pub fn doctor_json_report() -> Result<serde_json::Value> {
    let report = doctor()?;
    Ok(serde_json::to_value(report)?)
}

pub fn print_doctor_text() -> Result<()> {
    let report = doctor()?;
    println!("FREE_GB={:.2}", report.free_gb);
    println!("LM_STUDIO_MODELS={}", report.lm_studio_models);
    println!();

    for row in report.roles {
        println!("ROLE={}", row.role);
        println!("MANIFEST_MODEL={}", row.manifest_model);
        println!("ENV_MODEL={}", row.env_model);
        println!("IN_SYNC={}", row.in_sync);
        println!("MODEL_AVAILABLE={}", row.model_available);
        println!("MODEL_ID={}", row.model_id);
        println!("BACKEND={}", row.backend);
        println!("PATH={}", row.path);
        println!("STATUS={}", row.status);
        println!("ERROR={}", row.error);
        println!("SWITCH_READY={}", row.switch_ready);
        println!("THRESHOLD_GB={:.2}", row.threshold_gb);
        println!();
    }

    Ok(())
}

pub fn list_all_models_v0() -> Result<Vec<V0ModelInfo>> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        let manifest = model_manifest::load_manifest()?;
        let data = manifest
            .models
            .into_iter()
            .filter(|m| m.enabled)
            .map(|m| V0ModelInfo {
                id: m.id,
                state: Some("loaded".to_string()),
                arch: Some(m.backend.unwrap_or_else(|| "mlx".to_string())),
                quantization: None,
            })
            .collect();
        return Ok(data);
    }

    let url = format!("{}/api/v0/models", LM_STUDIO_BASE);
    let resp = std::thread::spawn(move || reqwest::blocking::get(&url).and_then(|r| r.text()))
        .join()
        .map_err(|_| anyhow!("thread panic"))??;
    let parsed: V0ModelsResponse =
        serde_json::from_str(&resp).map_err(|e| anyhow!("list_all_models_v0 parse: {e}"))?;
    Ok(parsed.data)
}

pub fn list_loaded_models() -> Result<Vec<String>> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        let manifest = model_manifest::load_manifest()?;
        return Ok(manifest
            .models
            .into_iter()
            .filter(|m| m.enabled)
            .map(|m| m.id)
            .collect());
    }

    let manifest = model_manifest::load_manifest()?;
    let mlx_enabled: Vec<String> = manifest
        .models
        .into_iter()
        .filter(|m| m.enabled && m.backend.as_deref() == Some("mlx"))
        .map(|m| m.id)
        .collect();

    if !mlx_enabled.is_empty() {
        return Ok(mlx_enabled);
    }

    Ok(list_all_models_v0()?
        .into_iter()
        .filter(|m| m.state.as_deref() == Some("loaded"))
        .map(|m| m.id)
        .collect())
}

pub fn load_model(identifier: &str) -> Result<()> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        println!("LOAD_OK model={identifier}");
        return Ok(());
    }

    let url = format!("{}/api/v0/models/load", LM_STUDIO_BASE);
    let body = serde_json::json!({ "identifier": identifier });
    let id = identifier.to_string();
    std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(&url)
            .json(&body)
            .send()
    })
    .join()
    .map_err(|_| anyhow!("thread panic"))??;
    println!("LOAD_OK model={id}");
    Ok(())
}

pub fn unload_model(identifier: &str) -> Result<()> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        println!("UNLOAD_OK model={identifier}");
        return Ok(());
    }

    let url = format!("{}/api/v0/models/unload", LM_STUDIO_BASE);
    let body = serde_json::json!({ "identifier": identifier });
    let id = identifier.to_string();
    std::thread::spawn(move || {
        reqwest::blocking::Client::new()
            .post(&url)
            .json(&body)
            .send()
    })
    .join()
    .map_err(|_| anyhow!("thread panic"))??;
    println!("UNLOAD_OK model={id}");
    Ok(())
}

pub fn smart_switch(target_model: &str, required_gb: f64) -> Result<()> {
    let free_gb = free_memory_gb_estimate()?;
    println!(
        "SMART_SWITCH target={target_model} required_gb={required_gb:.2} free_gb={free_gb:.2}"
    );

    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        println!("ALREADY_LOADED model={target_model}");
        return Ok(());
    }

    let loaded = list_loaded_models()?;
    println!("LOADED_NOW {:?}", loaded);

    if loaded.contains(&target_model.to_string()) {
        println!("ALREADY_LOADED model={target_model}");
        return Ok(());
    }

    if free_gb < required_gb {
        for m in &loaded {
            println!("UNLOADING_TO_FREE model={m}");
            unload_model(m)?;
        }
        let free_after = free_memory_gb_estimate()?;
        if free_after < required_gb {
            return Err(anyhow!(
                "smart_switch: not enough memory after unload: {free_after:.2} GB < {required_gb:.2} GB"
            ));
        }
    }

    load_model(target_model)
}

pub fn print_loaded_models() -> Result<()> {
    let loaded = list_loaded_models()?;
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
    let all = list_all_models_v0()?;
    let free_gb = free_memory_gb_estimate()?;
    println!("FREE_GB={free_gb:.2}");
    if all.is_empty() {
        println!("NO_MODELS");
    } else {
        for m in all {
            println!(
                "MODEL id={} state={} arch={} quantization={}",
                m.id,
                m.state.unwrap_or_else(|| "<unknown>".to_string()),
                m.arch.unwrap_or_else(|| "<unknown>".to_string()),
                m.quantization.unwrap_or_else(|| "<unknown>".to_string())
            );
        }
    }
    Ok(())
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
        let raw = doctor_json_report().unwrap();
        assert!(raw.get("free_gb").is_some());
        assert!(raw.get("lm_studio_models").is_some());
        assert!(raw.get("roles").is_some());
        assert!(raw["roles"].is_array());

        if let Some(first) = raw["roles"].as_array().and_then(|rows| rows.first()) {
            assert!(first.get("role").is_some());
            assert!(first.get("manifest_model").is_some());
            assert!(first.get("env_model").is_some());
            assert!(first.get("in_sync").is_some());
            assert!(first.get("model_available").is_some());
            assert!(first.get("model_id").is_some());
            assert!(first.get("backend").is_some());
            assert!(first.get("path").is_some());
            assert!(first.get("status").is_some());
            assert!(first.get("error").is_some());
            assert!(first.get("switch_ready").is_some());
            assert!(first.get("threshold_gb").is_some());
        }
    }
}
