use std::path::Path;

/// Resolve a path relative to workspace, preventing directory traversal.
///
/// Canonical authorization helper for filesystem confinement: the workspace
/// is canonicalized, symlinks in the target are resolved, and the canonical
/// result must stay under the workspace root. Reused by the patch-apply
/// tool (P2) so there is exactly one confinement implementation.
pub(crate) fn resolve_safe(path: &str, workspace: &str) -> Result<std::path::PathBuf, String> {
    let ws = Path::new(workspace)
        .canonicalize()
        .map_err(|e| format!("workspace error: {e}"))?;
    let target = if Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        ws.join(path)
    };

    // If file exists, canonicalize it directly
    if let Ok(canonical) = target.canonicalize() {
        if !canonical.starts_with(&ws) {
            return Err("Path escapes workspace root".to_string());
        }
        return Ok(canonical);
    }

    // File doesn't exist yet — canonicalize the parent directory
    if let Some(parent) = target.parent() {
        if let Ok(parent_canonical) = parent.canonicalize() {
            let file_name = target.file_name().ok_or("invalid path")?;
            let canonical = parent_canonical.join(file_name);
            if !canonical.starts_with(&ws) {
                return Err("Path escapes workspace root".to_string());
            }
            return Ok(canonical);
        }
    }

    // Fallback: neither the target nor its parent exists yet, so nothing can
    // be canonicalized. Normalize '..' lexically before the prefix check —
    // Path::starts_with compares component-wise and does NOT resolve '..',
    // so checking the raw target would let "ws/x/../../etc/passwd" through
    // and the subsequent fs call would resolve it at OS level.
    let normalized = lexical_normalize(&target);
    if !normalized.starts_with(&ws) {
        return Err("Path escapes workspace root".to_string());
    }
    Ok(normalized)
}

/// Lexically resolve `.` and `..` without touching the filesystem.
///
/// Only used when canonicalization is impossible because the target and its
/// parent do not exist yet. Symlinks are NOT resolved here; the canonicalize
/// branches in `resolve_safe` above are what handle those.
fn lexical_normalize(target: &Path) -> std::path::PathBuf {
    use std::path::Component;

    let mut out = std::path::PathBuf::new();
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // pop() fails at a root ("/.." == "/") or on an empty relative
                // path. Keep ".." only in the relative case, so that escaping
                // above the workspace stays visible to the starts_with check.
                if !out.pop() && !out.is_absolute() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

pub async fn read_file(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing 'path'")?;
    let canonical = resolve_safe(path, workspace)?;
    let content = std::fs::read_to_string(&canonical).map_err(|e| format!("read error: {e}"))?;
    Ok(serde_json::json!({
        "path": canonical.to_string_lossy(),
        "content": content,
        "lines": content.lines().count(),
        "bytes": content.len(),
    }))
}

pub async fn write_file(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing 'path'")?;
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or("missing 'content'")?;
    let canonical = resolve_safe(path, workspace)?;

    // Ensure parent directory exists
    if let Some(parent) = canonical.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir error: {e}"))?;
    }

    let bytes = content.len();
    std::fs::write(&canonical, content).map_err(|e| format!("write error: {e}"))?;
    Ok(serde_json::json!({
        "path": canonical.to_string_lossy(),
        "bytes_written": bytes,
    }))
}

pub async fn edit_file(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing 'path'")?;
    let old = args
        .get("old_string")
        .and_then(|v| v.as_str())
        .ok_or("missing 'old_string'")?;
    let new = args
        .get("new_string")
        .and_then(|v| v.as_str())
        .ok_or("missing 'new_string'")?;
    let canonical = resolve_safe(path, workspace)?;

    let content = std::fs::read_to_string(&canonical).map_err(|e| format!("read error: {e}"))?;
    let count = content.matches(old).count();
    if count == 0 {
        return Err("old_string not found in file".to_string());
    }
    if count > 1 && args.get("replace_all").and_then(|v| v.as_bool()) != Some(true) {
        return Err(format!(
            "Found {count} matches for old_string — provide more context or set replace_all"
        ));
    }

    let new_content = if args.get("replace_all").and_then(|v| v.as_bool()) == Some(true) {
        content.replace(old, new)
    } else {
        content.replacen(old, new, 1)
    };

    let bytes = new_content.len();
    std::fs::write(&canonical, new_content).map_err(|e| format!("write error: {e}"))?;
    Ok(serde_json::json!({
        "path": canonical.to_string_lossy(),
        "replacements": if args.get("replace_all").and_then(|v| v.as_bool()) == Some(true) { count } else { 1 },
        "bytes_written": bytes,
    }))
}

pub async fn list_directory(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let canonical = resolve_safe(path, workspace)?;

    let entries: Vec<serde_json::Value> = std::fs::read_dir(&canonical)
        .map_err(|e| format!("read_dir error: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| {
            let file_type = e
                .file_type()
                .map(|ft| {
                    if ft.is_dir() {
                        "dir"
                    } else if ft.is_symlink() {
                        "symlink"
                    } else {
                        "file"
                    }
                })
                .unwrap_or("unknown");
            let name = e.file_name().to_string_lossy().to_string();
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            serde_json::json!({
                "name": name,
                "type": file_type,
                "size": size,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "path": canonical.to_string_lossy(),
        "count": entries.len(),
        "entries": entries,
    }))
}

pub async fn find_files(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let root = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let canonical = resolve_safe(root, workspace)?;

    let mut files = Vec::new();

    // Simple recursive find matching pattern
    fn walk(dir: &Path, pattern: &str, files: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, pattern, files);
                } else if let Some(name) = path.file_name() {
                    if glob_match(pattern, &name.to_string_lossy()) {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    // Use simple pattern matching
    let simple_pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("*");
    walk(&canonical, simple_pattern, &mut files);
    files.truncate(100); // limit results

    Ok(serde_json::json!({
        "pattern": simple_pattern,
        "count": files.len(),
        "files": files,
    }))
}

fn glob_match(pattern: &str, name: &str) -> bool {
    // Simple glob: * matches any, ? matches one char
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return name == pattern;
    }
    let mut pos = 0;
    for part in &parts {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = name[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }
    true
}

pub async fn grep_files(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let pattern = args
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or("missing 'pattern'")?;
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let include = args.get("include").and_then(|v| v.as_str());
    let canonical = resolve_safe(path, workspace)?;

    let re = regex::Regex::new(pattern).map_err(|e| format!("invalid regex: {e}"))?;
    let mut matches = Vec::new();

    fn walk(
        dir: &Path,
        re: &regex::Regex,
        include: Option<&str>,
        matches: &mut Vec<serde_json::Value>,
    ) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, re, include, matches);
                } else {
                    if let Some(ext) = include {
                        if let Some(file_ext) = path.extension() {
                            if file_ext != ext.trim_start_matches('.') {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        for (i, line) in content.lines().enumerate() {
                            if re.is_match(line) {
                                matches.push(serde_json::json!({
                                    "file": path.to_string_lossy(),
                                    "line": i + 1,
                                    "content": line.trim(),
                                }));
                                if matches.len() >= 50 {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    walk(&canonical, &re, include, &mut matches);

    Ok(serde_json::json!({
        "pattern": pattern,
        "matches": matches.len(),
        "results": matches,
    }))
}

pub async fn get_file_info(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("missing 'path'")?;
    let canonical = resolve_safe(path, workspace)?;

    let meta = std::fs::metadata(&canonical).map_err(|e| format!("metadata error: {e}"))?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(serde_json::json!({
        "path": canonical.to_string_lossy(),
        "is_file": meta.is_file(),
        "is_dir": meta.is_dir(),
        "size": meta.len(),
        "modified_unix": modified,
        "readonly": meta.permissions().readonly(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Disposable workspace per test — never the user's repository.
    fn fresh_workspace(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dak_file_tools_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("create workspace");
        dir.canonicalize().expect("canonical workspace")
    }

    #[test]
    fn lexical_normalize_collapses_parent_dir_components() {
        let p = Path::new("/ws/sub/../other/./f.txt");
        assert_eq!(lexical_normalize(p), Path::new("/ws/other/f.txt"));
    }

    #[test]
    fn lexical_normalize_keeps_escape_visible_for_relative_paths() {
        // A relative path that climbs above its start must not be silently
        // collapsed to "/", otherwise the starts_with check would pass.
        assert_eq!(lexical_normalize(Path::new("../x")), Path::new("../x"));
    }

    #[test]
    fn resolve_safe_rejects_dotdot_escape_through_the_fallback_branch() {
        let ws = fresh_workspace("fallback_escape");
        // Neither the target nor its parent exists, so canonicalization fails
        // and resolve_safe reaches the lexical fallback.
        let err = resolve_safe("missing_dir/../../outside.txt", &ws.to_string_lossy())
            .expect_err("traversal must be rejected");
        assert!(
            err.contains("escapes workspace"),
            "expected an escape error, got: {err}"
        );
        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn resolve_safe_still_allows_new_file_in_a_new_subdirectory() {
        let ws = fresh_workspace("new_subdir");
        // The fallback must stay usable for legitimate creation of a file
        // whose parent directory does not exist yet.
        let got = resolve_safe("brand_new_dir/report.md", &ws.to_string_lossy())
            .expect("new file inside the workspace must resolve");
        assert!(got.starts_with(&ws), "resolved outside workspace: {got:?}");
        assert_eq!(got.file_name().unwrap(), "report.md");
        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn resolve_safe_rejects_absolute_path_outside_workspace() {
        let ws = fresh_workspace("absolute_outside");
        let err = resolve_safe("/etc/passwd", &ws.to_string_lossy())
            .expect_err("absolute path outside workspace must be rejected");
        assert!(err.contains("escapes workspace"), "got: {err}");
        std::fs::remove_dir_all(&ws).ok();
    }

    #[test]
    fn resolve_safe_accepts_existing_file_inside_workspace() {
        let ws = fresh_workspace("existing_inside");
        std::fs::write(ws.join("ok.txt"), b"x").expect("write fixture");
        let got = resolve_safe("ok.txt", &ws.to_string_lossy()).expect("must resolve");
        assert_eq!(got, ws.join("ok.txt"));
        std::fs::remove_dir_all(&ws).ok();
    }
}
