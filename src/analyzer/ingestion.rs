//! Repo ingestion — honest read-only inventory of a workspace.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One inventoried file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path relative to the workspace root (POSIX separators).
    pub rel_path: String,
    /// Detected language family (`python`, `rust`, `javascript`, …,
    /// `unknown`).
    pub language: String,
    pub bytes: u64,
    /// BLAKE3 of the content — lets later layers detect drift cheaply.
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInventory {
    pub root: String,
    /// Sorted by rel_path (deterministic).
    pub files: Vec<FileEntry>,
}

impl WorkspaceInventory {
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn by_language(&self, lang: &str) -> Vec<&FileEntry> {
        self.files.iter().filter(|f| f.language == lang).collect()
    }
}

/// Classify by extension — intentionally simple and deterministic.
fn detect_language(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("py") => "python",
        Some("rs") => "rust",
        Some("js" | "jsx" | "mjs") => "javascript",
        Some("ts" | "tsx") => "typescript",
        Some("java" | "kt" | "kts") => "jvm",
        Some("go") => "go",
        Some("md" | "rst" | "txt") => "text",
        Some("json" | "toml" | "yaml" | "yml") => "config",
        _ => "unknown",
    }
}

/// Directories never scanned (build outputs, VCS internals, caches, and
/// the analyzer's own artifact directories — evidence manifests must not
/// ingest analyzer output).
fn skip_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | "node_modules"
            | "__pycache__"
            | ".venv"
            | "venv"
            | ".mypy_cache"
            | ".pytest_cache"
            | "dist"
            | "build"
            | ".idea"
            | ".vscode"
            | "analyzer_out"
            | "analyzer_logs"
    )
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<FileEntry>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p.is_dir() {
                return false;
            }
            !name.starts_with('.')
        })
        .collect();
    paths.sort();
    for path in paths {
        let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(FileEntry {
            rel_path: rel,
            language: detect_language(&path).to_string(),
            bytes: bytes.len() as u64,
            content_hash: blake3::hash(&bytes).to_hex().to_string(),
        });
    }
    // Recurse into subdirectories (sorted for determinism).
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !skip_dir(p.file_name().and_then(|n| n.to_str()).unwrap_or("")))
        .collect();
    subdirs.sort();
    for sub in subdirs {
        walk(root, &sub, out)?;
    }
    Ok(())
}

/// Read-only scan: builds a deterministic inventory of `path`.
pub fn scan_workspace(path: &Path) -> Result<WorkspaceInventory, String> {
    if !path.is_dir() {
        return Err(format!("workspace is not a directory: {}", path.display()));
    }
    let mut files = Vec::new();
    walk(path, path, &mut files)?;
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(WorkspaceInventory {
        root: path.to_string_lossy().to_string(),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy_workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analyzer_examples/billing_python")
    }

    #[test]
    fn scan_sees_billing_module_and_tests() {
        let inv = scan_workspace(&toy_workspace()).unwrap();
        let rels: Vec<&str> = inv.files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(
            rels.contains(&"billing/fees.py"),
            "fees.py missing: {rels:?}"
        );
        assert!(rels.contains(&"test_fees.py"), "test_fees.py missing");
        assert!(inv.by_language("python").len() >= 3);
    }

    #[test]
    fn scan_is_deterministic_and_hashed() {
        let a = scan_workspace(&toy_workspace()).unwrap();
        let b = scan_workspace(&toy_workspace()).unwrap();
        assert_eq!(a, b, "inventory must be deterministic");
        assert!(a.files.iter().all(|f| f.content_hash.len() == 64));
    }

    #[test]
    fn scan_rejects_missing_dir() {
        assert!(scan_workspace(Path::new("/nonexistent/dak_analyzer_dir")).is_err());
    }
}
