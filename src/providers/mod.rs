use anyhow::Result;
use std::sync::OnceLock;

pub mod storage;

pub trait FilesystemProvider: Send + Sync {
    fn read_to_string(&self, path: &str) -> Result<String>;
    fn write(&self, path: &str, contents: &str) -> Result<()>;
    fn create_dir_all(&self, path: &str) -> Result<()>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmResponse {
    pub text: String,
    pub model_name: String,
    pub model_version: Option<String>,
}

pub trait LlmProvider: Send + Sync {
    fn coding_assistant(&self, prompt: &str) -> Result<String>;
    fn execute_llm(&self, prompt: &str, model_override: Option<&str>) -> Result<LlmResponse>;
    fn embed_text(&self, prompt: &str) -> Result<Vec<f32>>;
}

pub struct DefaultFilesystem;
impl FilesystemProvider for DefaultFilesystem {
    fn read_to_string(&self, path: &str) -> Result<String> {
        std::fs::read_to_string(path).map_err(Into::into)
    }
    fn write(&self, path: &str, contents: &str) -> Result<()> {
        std::fs::write(path, contents).map_err(Into::into)
    }
    fn create_dir_all(&self, path: &str) -> Result<()> {
        std::fs::create_dir_all(path).map_err(Into::into)
    }
}

pub struct DefaultLlm;
impl LlmProvider for DefaultLlm {
    fn coding_assistant(&self, prompt: &str) -> Result<String> {
        self.execute_llm(prompt, None).map(|r| r.text)
    }

    fn execute_llm(&self, prompt: &str, model_override: Option<&str>) -> Result<LlmResponse> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let model_name = model_override.map(|s| s.to_string()).unwrap_or_else(|| {
                    crate::model_registry::resolve_model(
                        crate::model_registry::ModelPurpose::CodingAssistant,
                    )
                    .map(|cfg| cfg.model)
                    .unwrap_or_else(|_| "unknown".to_string())
                });

                let model_version = std::env::var("OPENAI_MODEL_VERSION").ok();

                let text = crate::llm::chat_with_model_override(
                    crate::model_registry::ModelPurpose::CodingAssistant,
                    "You are a concise coding assistant.",
                    prompt,
                    model_override,
                )
                .await?;

                Ok(LlmResponse {
                    text,
                    model_name,
                    model_version,
                })
            })
        })
    }

    fn embed_text(&self, prompt: &str) -> Result<Vec<f32>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(crate::embeddings::embed_text(prompt))
        })
    }
}

static FILESYSTEM_PROVIDER: OnceLock<Box<dyn FilesystemProvider>> = OnceLock::new();
static LLM_PROVIDER: OnceLock<Box<dyn LlmProvider>> = OnceLock::new();
static STORAGE_PROVIDER: OnceLock<Box<dyn storage::StorageProvider>> = OnceLock::new();

pub fn get_filesystem() -> &'static dyn FilesystemProvider {
    FILESYSTEM_PROVIDER
        .get_or_init(|| Box::new(DefaultFilesystem))
        .as_ref()
}

pub fn get_llm() -> &'static dyn LlmProvider {
    LLM_PROVIDER.get_or_init(|| Box::new(DefaultLlm)).as_ref()
}

pub fn get_storage() -> &'static dyn storage::StorageProvider {
    STORAGE_PROVIDER
        .get_or_init(|| Box::new(storage::DefaultStorage::new()))
        .as_ref()
}

pub fn register_filesystem(provider: Box<dyn FilesystemProvider>) {
    let _ = FILESYSTEM_PROVIDER.set(provider);
}

pub fn register_llm(provider: Box<dyn LlmProvider>) {
    let _ = LLM_PROVIDER.set(provider);
}

pub fn register_storage(provider: Box<dyn storage::StorageProvider>) {
    let _ = STORAGE_PROVIDER.set(provider);
}
