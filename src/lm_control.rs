use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::process::Command;

use crate::model_manifest;

pub mod policy;

use policy::switch_plan;

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelInfo>,
}

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

pub fn list_models() -> Result<Vec<String>> {
    if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
        let manifest = model_manifest::load_manifest()?;
        return Ok(
            manifest
                .models
                .into_iter()
                .filter(|m| m.enabled)
                .map(|m| m.id)
                .collect(),
        );
    }

    let output = Command::new("sh")
        .arg("-lc")
        .arg("curl -s http://127.0.0.1:1234/v1/models")
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
    let models = list_models().unwrap_or_default();
    let rows = model_manifest::current_model_statuses()?;

    let mut roles = Vec::new();
    for row in rows {
        let threshold = model_manifest::threshold_gb_for_ram_class(
            &model_manifest::best_enabled_model_for_role(&row.role)?.ram_class,
        )?;
        let available = models.iter().any(|m| m == &row.manifest_model);
        roles.push(DoctorRoleReport {
            role: row.role,
            manifest_model: row.manifest_model,
            env_model: row.env_model.unwrap_or_else(|| "<missing>".to_string()),
            in_sync: row.in_sync,
            model_available: available,
            switch_ready: free_gb >= threshold && available,
            threshold_gb: threshold,
        });
    }

    Ok(DoctorReport {
        free_gb,
        lm_studio_models: models.len(),
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

    if !row.switch_ready {
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

pub fn doctor_json_report() -> Result<serde_json::Value> {
    let report = doctor()?;
    Ok(serde_json::to_value(report)?)
}

#[allow(dead_code)]
pub fn print_doctor_json() -> Result<()> {
    let report = doctor_json_report()?;
    let envelope = crate::cli_json::command_report("doctor-json", report);
    crate::cli_json::print_json_report(&envelope);
    Ok(())
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
        println!("SWITCH_READY={}", row.switch_ready);
        println!("THRESHOLD_GB={:.2}", row.threshold_gb);
        println!();
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
        let report = crate::cli_json::command_report("doctor-json", doctor_json_report().unwrap());

        assert_eq!(report["ok"], true);
        assert_eq!(report["schema_version"], "cli-json-v1");
        assert_eq!(report["command"], "doctor-json");
        assert!(report["report"].get("free_gb").is_some());
        assert!(report["report"].get("lm_studio_models").is_some());
        assert!(report["report"].get("roles").is_some());
        assert!(report["report"]["roles"].is_array());

        if let Some(first) = report["report"]["roles"].as_array().and_then(|rows| rows.first()) {
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
