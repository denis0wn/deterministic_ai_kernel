use std::sync::Once;

#[allow(dead_code)]
pub fn with_mock_lm_backend<F: FnOnce()>(f: F) {
    let prev_backend = std::env::var("DAK_LM_BACKEND").ok();
    let prev_mem = std::env::var("DAK_FREE_GB_OVERRIDE").ok();
    std::env::set_var("DAK_LM_BACKEND", "mock");
    std::env::set_var("DAK_FREE_GB_OVERRIDE", "16.0");
    f();
    match prev_backend {
        Some(v) => std::env::set_var("DAK_LM_BACKEND", v),
        None => std::env::remove_var("DAK_LM_BACKEND"),
    }
    match prev_mem {
        Some(v) => std::env::set_var("DAK_FREE_GB_OVERRIDE", v),
        None => std::env::remove_var("DAK_FREE_GB_OVERRIDE"),
    }
}

// ── Mock LLM Provider ──────────────────────────────────────────────────────
// A deterministic mock LLM for testing LLM-dependent code paths without
// requiring a live server. Returns canned responses based on prompt content.

pub struct MockLlm {
    pub embedding_dim: usize,
}

impl Default for MockLlm {
    fn default() -> Self {
        Self { embedding_dim: 128 }
    }
}

impl deterministic_ai_kernel::providers::LlmProvider for MockLlm {
    fn coding_assistant(&self, prompt: &str) -> anyhow::Result<String> {
        Ok(format!(
            "mock-coding-response-to: {}",
            &prompt[..prompt.len().min(50)]
        ))
    }

    fn execute_llm(
        &self,
        prompt: &str,
        model_override: Option<&str>,
    ) -> anyhow::Result<deterministic_ai_kernel::providers::LlmResponse> {
        let model = model_override.unwrap_or("mock-model-v1");
        Ok(deterministic_ai_kernel::providers::LlmResponse {
            text: format!("mock-llm-response-to: {}", &prompt[..prompt.len().min(50)]),
            model_name: model.to_string(),
            model_version: Some("mock-1.0".to_string()),
        })
    }

    fn embed_text(&self, prompt: &str) -> anyhow::Result<Vec<f32>> {
        // Deterministic embedding: hash-based values
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        prompt.hash(&mut hasher);
        let seed = hasher.finish();

        let mut embedding = Vec::with_capacity(self.embedding_dim);
        for i in 0..self.embedding_dim {
            let val = ((seed.wrapping_add(i as u64) as f64) / (u64::MAX as f64)) * 2.0 - 1.0;
            embedding.push(val as f32);
        }
        Ok(embedding)
    }
}

static MOCK_LLM_INIT: Once = Once::new();

/// Register the mock LLM provider. Safe to call multiple times — only the first
/// call takes effect. Must be called before any code calls get_llm().
pub fn register_mock_llm() {
    MOCK_LLM_INIT.call_once(|| {
        deterministic_ai_kernel::providers::register_llm(Box::new(MockLlm::default()));
    });
}

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("mock_llm_{}_{}.db", test_name, nanos))
}

pub fn cleanup_db(db: &PathBuf) {
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_file(format!("{}-wal", db.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db.display()));
}
