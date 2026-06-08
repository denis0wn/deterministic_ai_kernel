use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::lm_control;
use crate::model_registry::{resolve_model, ModelPurpose};

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
    lm_control::auto_route("embeddings")?;

    let config = resolve_model(ModelPurpose::CodingAssistant)?;
    let model = std::env::var("OPENAI_MODEL_EMBEDDINGS")
        .map_err(|_| anyhow!("OPENAI_MODEL_EMBEDDINGS is not set"))?;

    let url = format!("{}/embeddings", config.base_url.trim_end_matches('/'));
    let req = EmbeddingsRequest {
        model,
        input: vec![input.to_string()],
    };

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .bearer_auth(&config.api_key)
        .json(&req)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(anyhow!("embeddings request failed with status {}: {}", status, body));
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
