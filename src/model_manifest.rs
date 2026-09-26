use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelManifest {
    pub models: Vec<ManifestModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestModel {
    pub id: String,
    pub role: String,
    pub priority: u32,
    pub ram_class: String,
    pub enabled: bool,
    pub notes: String,
    pub system_prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentModelStatus {
    pub role: String,
    pub env_key: String,
    pub manifest_model: String,
    pub env_model: Option<String>,
    pub in_sync: bool,
}

pub fn load_manifest() -> Result<ModelManifest> {
    let text = fs::read_to_string("config/model_manifest.json")?;
    let manifest: ModelManifest = serde_json::from_str(&text)?;
    Ok(manifest)
}

pub fn print_manifest() -> Result<()> {
    let manifest = load_manifest()?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

pub fn best_enabled_model_for_role(role: &str) -> Result<ManifestModel> {
    let manifest = load_manifest()?;
    manifest
        .models
        .iter()
        .filter(|m| m.enabled && m.role == role)
        .min_by_key(|m| m.priority)
        .cloned()
        .ok_or_else(|| anyhow!("no enabled model found for role {:?}", role))
}

pub fn env_key_for_role(role: &str) -> Result<&'static str> {
    match role {
        "coding_assistant" => Ok("OPENAI_MODEL_CODING_ASSISTANT"),
        "task_planning" => Ok("OPENAI_MODEL_TASK_PLANNING"),
        "embeddings" => Ok("OPENAI_MODEL_EMBEDDINGS"),
        "code_review" => Ok("OPENAI_MODEL_CODING_ASSISTANT"),
        "coding_fallback" => Ok("OPENAI_MODEL_CODING_ASSISTANT"),
        "task_planning_fallback" => Ok("OPENAI_MODEL_TASK_PLANNING"),
        _ => Err(anyhow!("unsupported manifest role {:?}", role)),
    }
}

pub fn sync_env_for_role(role: &str) -> Result<String> {
    let model = best_enabled_model_for_role(role)?;
    let env_key = env_key_for_role(role)?;

    let env_path = ".env";
    let mut text = fs::read_to_string(env_path).unwrap_or_default();
    let prefix = format!("{env_key}=");

    if text.lines().any(|line| line.starts_with(&prefix)) {
        text = text
            .lines()
            .map(|line| {
                if line.starts_with(&prefix) {
                    format!("{env_key}={}", model.id)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    } else {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("{env_key}={}\n", model.id));
    }

    // In mock/test mode, skip writing .env (file may be quarantine-locked)
    if std::env::var("DAK_LM_BACKEND").as_deref() != Ok("mock") {
        // Atomic write: temp file + rename prevents corruption on crash.
        let dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let tmp = tempfile::NamedTempFile::new_in(&dir)?;
        std::fs::write(tmp.path(), &text)?;
        tmp.persist(env_path)?;
    }
    Ok(model.id)
}

pub fn threshold_gb_for_ram_class(ram_class: &str) -> Result<f64> {
    match ram_class {
        "light" => Ok(2.0),
        "medium" => Ok(6.0),
        "heavy" => Ok(10.0),
        other => Err(anyhow!("unsupported ram_class {:?}", other)),
    }
}

pub fn system_prompt_for_role(role: &str) -> Result<String> {
    let model = best_enabled_model_for_role(role)?;
    Ok(model.system_prompt)
}
fn read_env_value(env_key: &str) -> Result<Option<String>> {
    // Process env wins over the .env file (standard dotenvy precedence);
    // the file alone is absent on fresh checkouts (CI) and in tests.
    if let Ok(value) = std::env::var(env_key) {
        return Ok(Some(value));
    }
    let text = fs::read_to_string(".env").unwrap_or_default();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix(&format!("{env_key}=")) {
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

pub fn current_model_statuses() -> Result<Vec<CurrentModelStatus>> {
    let roles = [
        "coding_assistant",
        "task_planning",
        "code_review",
        "embeddings",
    ];
    let mut out = Vec::new();

    for role in roles {
        let manifest_model = best_enabled_model_for_role(role)?;
        let env_key = env_key_for_role(role)?.to_string();
        let env_model = read_env_value(&env_key)?;
        let in_sync = env_model.as_deref() == Some(manifest_model.id.as_str());

        out.push(CurrentModelStatus {
            role: role.to_string(),
            env_key,
            manifest_model: manifest_model.id,
            env_model,
            in_sync,
        });
    }

    Ok(out)
}

pub fn print_current_models() -> Result<()> {
    for row in current_model_statuses()? {
        println!("ROLE={}", row.role);
        println!("ENV_KEY={}", row.env_key);
        println!("MANIFEST_MODEL={}", row.manifest_model);
        println!(
            "ENV_MODEL={}",
            row.env_model.unwrap_or_else(|| "<missing>".to_string())
        );
        println!("IN_SYNC={}", row.in_sync);
        println!();
    }
    Ok(())
}

pub fn sync_all_roles() -> Result<()> {
    for role in [
        "coding_assistant",
        "task_planning",
        "code_review",
        "embeddings",
    ] {
        let model = sync_env_for_role(role)?;
        println!("SYNCED role={} model={}", role, model);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_loads() {
        let manifest = load_manifest().expect("failed to load manifest");
        assert!(!manifest.models.is_empty());
    }

    #[test]
    fn task_planning_model_is_present() {
        let manifest = load_manifest().expect("failed to load manifest");
        assert!(manifest.models.iter().any(|m| {
            m.id == "/Users/denissmoliakov/Models/ministral-14b-reasoning" && m.role == "task_planning"
        }));
    }

    #[test]
    fn embedding_model_is_not_marked_as_chat_role() {
        let manifest = load_manifest().expect("failed to load manifest");
        let embedding = manifest
            .models
            .iter()
            .find(|m| m.role == "embeddings")
            .expect("embedding model not found in manifest");

        assert_eq!(embedding.role, "embeddings");
    }

    #[test]
    fn best_enabled_task_planning_model_prefers_priority_one() {
        let model =
            best_enabled_model_for_role("task_planning").expect("best model for role not found");
        assert_eq!(model.id, "/Users/denissmoliakov/Models/ministral-14b-reasoning");
    }

    #[test]
    fn threshold_mapping_is_stable() {
        assert_eq!(
            threshold_gb_for_ram_class("light").expect("missing light ram class"),
            2.0
        );
        assert_eq!(
            threshold_gb_for_ram_class("medium").expect("missing medium ram class"),
            6.0
        );
        assert_eq!(
            threshold_gb_for_ram_class("heavy").expect("missing heavy ram class"),
            10.0
        );
    }

    #[test]
    fn current_models_reports_known_roles() {
        let rows = current_model_statuses().expect("failed to get current model statuses");
        assert!(rows.iter().any(|r| r.role == "coding_assistant"));
        assert!(rows.iter().any(|r| r.role == "task_planning"));
        assert!(rows.iter().any(|r| r.role == "embeddings"));
    }
}
