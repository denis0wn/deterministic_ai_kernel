use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::lm_control;

#[derive(Serialize)]
struct EmbeddingsRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}

pub async fn embed_text(input: &str) -> Result<Vec<f32>> {
    let mgr = crate::runtime_manager::EmbeddingRuntimeManager::load()
        .map_err(|e| anyhow!("Failed to load embedding runtime manager: {}", e))?;

    mgr.ensure_running()
        .map_err(|e| anyhow!("Failed to start embedding runtime: {}", e))?;

    lm_control::auto_route("embeddings")?;

    let (endpoint, model) = match crate::runtime_manager::RuntimeManager::load() {
        Ok(mgr) => {
            let embed_cfg = mgr.config().embeddings.as_ref()
                .ok_or_else(|| anyhow!("Embeddings not configured in config/runtime.json"))?;
            if embed_cfg.provider == "mock" || std::env::var("DAK_LM_BACKEND").as_deref() == Ok("mock") {
                return Ok(vec![0.1f32; 1536]);
            }
            (embed_cfg.endpoint.clone(), embed_cfg.model.clone())
        }
        Err(_) => {
            if std::env::var("DAK_LM_BACKEND").as_deref() == Ok("mock") {
                return Ok(vec![0.1f32; 1536]);
            }
            let base_url = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_string());
            let model = std::env::var("OPENAI_MODEL_EMBEDDINGS")
                .map_err(|_| anyhow!("OPENAI_MODEL_EMBEDDINGS is not set"))?;
            (base_url, model)
        }
    };

    let url = format!("{}/embeddings", endpoint.trim_end_matches('/'));
    let req = EmbeddingsRequest {
        model,
        input: vec![input.to_string()],
    };

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .json(&req)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(anyhow!(
            "embeddings request failed with status {}: {}",
            status,
            body
        ));
    }

    let parsed: EmbeddingsResponse = serde_json::from_str(&body)?;
    let vector = parsed
        .data
        .into_iter()
        .next()
        .map(|x| x.embedding)
        .ok_or_else(|| anyhow!("embeddings response did not contain a vector"))?;

    Ok(vector)
}

pub async fn embeddings_smoke() -> Result<()> {
    let v = embed_text("deterministic kernel embeddings smoke test").await?;
    if v.is_empty() {
        return Err(anyhow!("embeddings smoke failed: empty vector"));
    }

    println!("EMBEDDINGS_SMOKE_OK");
    println!("VECTOR_DIM={}", v.len());
    Ok(())
}
