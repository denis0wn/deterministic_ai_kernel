use serde::{Deserialize, Serialize};

/// Safety classification for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSafety {
    /// Read-only, no side effects, no confirmation needed.
    ReadOnly,
    /// Local side effects (file writes), confirmation recommended.
    LocalMutating,
    /// External side effects (shell, network, OS), confirmation required.
    ExternalMutating,
}

/// A tool that can be invoked by the agent.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ToolCategory,
    pub safety: ToolSafety,
    pub confirmation_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCategory {
    File,
    Shell,
    Web,
    App,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolCategory::File => write!(f, "file"),
            ToolCategory::Shell => write!(f, "shell"),
            ToolCategory::Web => write!(f, "web"),
            ToolCategory::App => write!(f, "app"),
        }
    }
}

impl ToolSafety {
    pub fn label(self) -> &'static str {
        match self {
            ToolSafety::ReadOnly => "read-only",
            ToolSafety::LocalMutating => "local-mutating",
            ToolSafety::ExternalMutating => "external-mutating",
        }
    }
}

/// Status of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolStatus {
    Started,
    Running,
    AwaitingConfirmation,
    Success,
    Error(String),
    Cancelled,
}

/// A single tool invocation event for history/logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub status: ToolStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub confirmed: bool,
    pub timestamp: String,
}

/// Common envelope for tool execution results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: serde_json::Value,
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl ToolResult {
    pub fn ok(output: serde_json::Value, duration_ms: u64) -> Self {
        Self {
            success: true,
            output,
            error: None,
            duration_ms,
        }
    }

    pub fn err(error: String, duration_ms: u64) -> Self {
        Self {
            success: false,
            output: serde_json::Value::Null,
            error: Some(error),
            duration_ms,
        }
    }
}
