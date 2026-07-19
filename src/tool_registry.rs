use anyhow::{anyhow, Result};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedTool {
    FsReadFile,
    FsWriteFile,
    RepoApplyPatchCanonical,
    RepoRunTestsCargoAllTargets,
}

impl AllowedTool {
    pub fn from_binding(binding: &str) -> Option<Self> {
        match binding {
            "fs.read_file" => Some(Self::FsReadFile),
            "fs.write_file" => Some(Self::FsWriteFile),
            "repo.apply_patch.canonical" => Some(Self::RepoApplyPatchCanonical),
            "repo.run_tests.cargo_all_targets" => Some(Self::RepoRunTestsCargoAllTargets),
            _ => None,
        }
    }

    pub fn binding(&self) -> &'static str {
        match self {
            Self::FsReadFile => "fs.read_file",
            Self::FsWriteFile => "fs.write_file",
            Self::RepoApplyPatchCanonical => "repo.apply_patch.canonical",
            Self::RepoRunTestsCargoAllTargets => "repo.run_tests.cargo_all_targets",
        }
    }

    pub fn tool_version(&self) -> &'static str {
        "v1"
    }

    pub fn materialize_payload(&self, detail: Option<&str>) -> Result<serde_json::Value> {
        match self {
            Self::FsReadFile => {
                let path = detail
                    .and_then(|s| s.split_whitespace().next())
                    .ok_or_else(|| anyhow!("fs.read_file requires a path detail"))?;
                Ok(json!({
                    "binding": self.binding(),
                    "tool_version": self.tool_version(),
                    "path": path,
                }))
            }
            Self::FsWriteFile => {
                let raw = detail
                    .ok_or_else(|| anyhow!("fs.write_file requires path and content detail"))?;
                let mut parts = raw.splitn(2, " with ");
                let path = parts.next().unwrap_or("").trim();
                let content = parts.next().unwrap_or("").trim();
                if path.is_empty() || content.is_empty() {
                    return Err(anyhow!("fs.write_file requires 'path with content' detail"));
                }
                Ok(json!({
                    "binding": self.binding(),
                    "tool_version": self.tool_version(),
                    "path": path,
                    "content": content,
                }))
            }
            Self::RepoApplyPatchCanonical => Ok(json!({
                "binding": self.binding(),
                "tool_version": self.tool_version(),
                "profile": "canonical_repo_patch",
                "executable_tool": "repo.apply_patch.canonical",
            })),
            Self::RepoRunTestsCargoAllTargets => Ok(json!({
                "binding": self.binding(),
                "tool_version": self.tool_version(),
                "executable_tool": "cargo test --all-targets",
            })),
        }
    }
}

pub fn materialize_allowed_tool(binding: &str, detail: Option<&str>) -> Result<serde_json::Value> {
    let tool = AllowedTool::from_binding(binding)
        .ok_or_else(|| anyhow!("unknown tool binding: {binding}"))?;
    tool.materialize_payload(detail)
}
