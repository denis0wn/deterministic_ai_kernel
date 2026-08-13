use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionState {
    pub screen: String,
    pub nav_index: usize,
    pub history_index: usize,
    pub output_scroll: usize,
    pub search_query: String,
}

fn session_dir() -> PathBuf {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("config")
        .join("model_manifest.json");
    let project_dir = manifest_path.parent().unwrap().parent().unwrap();
    project_dir.join(".replay_os")
}

fn session_file() -> PathBuf {
    session_dir().join("session.json")
}

pub fn save_session(state: &SessionState) {
    let dir = session_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    let _ = fs::write(session_file(), serde_json::to_string_pretty(state).unwrap());
}

pub fn load_session() -> SessionState {
    let path = session_file();
    if !path.exists() {
        return SessionState::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}
