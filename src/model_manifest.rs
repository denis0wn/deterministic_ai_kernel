use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelManifest {
    pub models: Vec<ManifestModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestCapabilities {
    pub text: bool,
    pub reasoning: bool,
    pub code: bool,
    pub vision: bool,
    pub audio: bool,
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
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub capabilities: Option<ManifestCapabilities>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CurrentModelStatus {
    pub role: String,
    pub env_key: String,
    pub manifest_model: String,
    pub env_model: Option<String>,
    pub in_sync: bool,
    pub model_id: String,
    pub backend: String,
    pub path: String,
    pub status: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMlxModel {
    pub id: String,
    pub role: String,
    pub backend: String,
    pub path: PathBuf,
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

pub fn model_by_id(model_id: &str) -> Result<ManifestModel> {
    let manifest = load_manifest()?;
    manifest
        .models
        .iter()
        .find(|m| m.id == model_id)
        .cloned()
        .ok_or_else(|| anyhow!("model id not found in manifest: {}", model_id))
}

#[allow(dead_code)]
pub fn best_enabled_mlx_model_for_role(role: &str) -> Result<ManifestModel> {
    let model = best_enabled_model_for_role(role)?;
    ensure_mlx_model_shape(&model)?;
    Ok(model)
}

#[allow(dead_code)]
pub fn single_active_runtime_model() -> Result<ManifestModel> {
    let manifest = load_manifest()?;
    manifest
        .models
        .iter()
        .find(|m| m.enabled && m.backend.as_deref() == Some("mlx"))
        .cloned()
        .ok_or_else(|| anyhow!("no enabled MLX runtime model found"))
}

pub fn env_key_for_role(role: &str) -> Result<&'static str> {
    match role {
        "coding_assistant" => Ok("OPENAI_MODEL_CODING_ASSISTANT"),
        "task_planning" => Ok("OPENAI_MODEL_TASK_PLANNING"),
        "embeddings" => Ok("OPENAI_MODEL_EMBEDDINGS"),
        "code_review" => Ok("OPENAI_MODEL_CODE_REVIEW"),
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

    fs::write(env_path, text)?;
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
    let text = fs::read_to_string(".env").unwrap_or_default();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix(&format!("{env_key}=")) {
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

pub fn expand_model_path(raw: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/denissmoliakov".to_string());
    let expanded = if raw == "~" {
        home
    } else if let Some(rest) = raw.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        raw.to_string()
    };
    Ok(PathBuf::from(expanded))
}

fn ensure_required_mlx_files_exist(path: &Path) -> Result<()> {
    let required = [
        "config.json",
        "tokenizer.json",
        "model-00001-of-00002.safetensors",
        "model-00002-of-00002.safetensors",
    ];

    for file in required {
        let candidate = path.join(file);
        if !candidate.exists() {
            return Err(anyhow!(
                "required MLX model file missing: {}",
                candidate.display()
            ));
        }
    }

    Ok(())
}

fn ensure_mlx_model_shape(model: &ManifestModel) -> Result<()> {
    if !model.enabled {
        return Err(anyhow!("manifest model is disabled: {}", model.id));
    }

    if model.backend.as_deref() != Some("mlx") {
        return Err(anyhow!(
            "manifest model backend is not mlx: {} backend={:?}",
            model.id,
            model.backend
        ));
    }

    let raw_path = model
        .path
        .as_deref()
        .ok_or_else(|| anyhow!("manifest model path is missing: {}", model.id))?;

    let path = expand_model_path(raw_path)?;
    if !path.exists() {
        return Err(anyhow!(
            "manifest model path does not exist: {}",
            path.display()
        ));
    }

    if !path.is_dir() {
        return Err(anyhow!(
            "manifest model path is not a directory: {}",
            path.display()
        ));
    }

    ensure_required_mlx_files_exist(&path)?;
    Ok(())
}

pub fn verify_mlx_model_by_id(model_id: &str) -> Result<VerifiedMlxModel> {
    let model = model_by_id(model_id)?;
    ensure_mlx_model_shape(&model)?;
    let path = expand_model_path(
        model
            .path
            .as_deref()
            .ok_or_else(|| anyhow!("manifest model path is missing: {}", model.id))?,
    )?;

    Ok(VerifiedMlxModel {
        id: model.id,
        role: model.role,
        backend: "mlx".to_string(),
        path,
    })
}

#[allow(dead_code)]
pub fn verify_best_mlx_model_for_role(role: &str) -> Result<VerifiedMlxModel> {
    let model = best_enabled_mlx_model_for_role(role)?;
    let path = expand_model_path(
        model
            .path
            .as_deref()
            .ok_or_else(|| anyhow!("manifest model path is missing: {}", model.id))?,
    )?;

    Ok(VerifiedMlxModel {
        id: model.id,
        role: model.role,
        backend: "mlx".to_string(),
        path,
    })
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
        let selected = best_enabled_model_for_role(role)?;
        let env_key = env_key_for_role(role)?.to_string();
        let env_model = read_env_value(&env_key)?;
        let in_sync = env_model.as_deref() == Some(selected.id.as_str());

        let backend = selected
            .backend
            .clone()
            .unwrap_or_else(|| "<missing>".to_string());
        let path = selected
            .path
            .clone()
            .unwrap_or_else(|| "<missing>".to_string());

        let verification = if selected.backend.as_deref() == Some("mlx") {
            verify_mlx_model_by_id(&selected.id).map(|_| ())
        } else {
            Err(anyhow!(
                "selected model for role {:?} is not an mlx model: {}",
                role,
                selected.id
            ))
        };

        let (status, error) = match verification {
            Ok(()) => ("ready".to_string(), String::new()),
            Err(e) => ("error".to_string(), e.to_string()),
        };

        out.push(CurrentModelStatus {
            role: role.to_string(),
            env_key,
            manifest_model: selected.id.clone(),
            env_model,
            in_sync,
            model_id: selected.id,
            backend,
            path,
            status,
            error,
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
            row.env_model
                .clone()
                .unwrap_or_else(|| "<missing>".to_string())
        );
        println!("IN_SYNC={}", row.in_sync);
        println!("MODEL_ID={}", row.model_id);
        println!("BACKEND={}", row.backend);
        println!("PATH={}", row.path);
        println!("STATUS={}", row.status);
        println!("ERROR={}", row.error);
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
        let manifest = load_manifest().unwrap();
        assert!(!manifest.models.is_empty());
    }

    #[test]
    fn task_planning_model_is_present() {
        let manifest = load_manifest().unwrap();
        assert!(manifest.models.iter().any(|m| {
            m.id == "huihui-gemma-4-e2b-it-abliterated-mlx" && m.role == "task_planning"
        }));
    }

    #[test]
    fn embedding_model_is_not_marked_as_chat_role() {
        let manifest = load_manifest().unwrap();
        let embedding = manifest
            .models
            .iter()
            .find(|m| m.id == "text-embedding-nomic-embed-text-v1.5")
            .unwrap();

        assert_eq!(embedding.role, "embeddings");
    }

    #[test]
    fn best_enabled_task_planning_model_prefers_priority_one() {
        let model = best_enabled_model_for_role("task_planning").unwrap();
        assert_eq!(model.id, "google/gemma-4-12b-qat");
    }

    #[test]
    fn threshold_mapping_is_stable() {
        assert_eq!(threshold_gb_for_ram_class("light").unwrap(), 2.0);
        assert_eq!(threshold_gb_for_ram_class("medium").unwrap(), 6.0);
        assert_eq!(threshold_gb_for_ram_class("heavy").unwrap(), 10.0);
    }

    #[test]
    fn current_models_reports_known_roles() {
        let rows = current_model_statuses().unwrap();
        assert!(rows.iter().any(|r| r.role == "coding_assistant"));
        assert!(rows.iter().any(|r| r.role == "task_planning"));
        assert!(rows.iter().any(|r| r.role == "embeddings"));
    }

    #[test]
    fn tilde_expansion_works() {
        let path = expand_model_path("~/Models/gemma4-reasoning").unwrap();
        assert!(path.to_string_lossy().contains("/Models/gemma4-reasoning"));
    }

    #[test]
    fn verified_mlx_model_for_known_id_resolves_path() {
        let verified = verify_mlx_model_by_id("google/gemma-4-12b-qat").unwrap();
        assert_eq!(verified.backend, "mlx");
        assert!(verified.path.ends_with("gemma4-reasoning"));
    }
}
