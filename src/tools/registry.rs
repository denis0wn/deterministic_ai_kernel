use super::contract::{ToolCategory, ToolDef, ToolResult, ToolSafety};

/// Central tool registry — holds all available tools.
pub struct ToolRegistry {
    tools: Vec<ToolDef>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut reg = Self { tools: Vec::new() };
        reg.register_defaults();
        reg
    }

    fn register_defaults(&mut self) {
        // File tools
        self.register(ToolDef {
            name: "read_file",
            description: "Read file contents",
            category: ToolCategory::File,
            safety: ToolSafety::ReadOnly,
            confirmation_required: false,
        });
        self.register(ToolDef {
            name: "write_file",
            description: "Write content to file",
            category: ToolCategory::File,
            safety: ToolSafety::LocalMutating,
            confirmation_required: true,
        });
        self.register(ToolDef {
            name: "edit_file",
            description: "Edit file via exact string replacement",
            category: ToolCategory::File,
            safety: ToolSafety::LocalMutating,
            confirmation_required: true,
        });
        self.register(ToolDef {
            name: "list_directory",
            description: "List directory contents",
            category: ToolCategory::File,
            safety: ToolSafety::ReadOnly,
            confirmation_required: false,
        });
        self.register(ToolDef {
            name: "find_files",
            description: "Find files by glob pattern",
            category: ToolCategory::File,
            safety: ToolSafety::ReadOnly,
            confirmation_required: false,
        });
        self.register(ToolDef {
            name: "grep_files",
            description: "Search file contents by regex",
            category: ToolCategory::File,
            safety: ToolSafety::ReadOnly,
            confirmation_required: false,
        });
        self.register(ToolDef {
            name: "get_file_info",
            description: "Get file metadata (size, modified, type)",
            category: ToolCategory::File,
            safety: ToolSafety::ReadOnly,
            confirmation_required: false,
        });

        // Authorized patch application (P2): applies a kernel-validated
        // PatchV1 atomically inside the workspace. Mutating, therefore
        // confirmation-gated like every other mutating tool; the kernel
        // effect path supplies its own policy confirmation after full
        // kernel-side validation.
        self.register(ToolDef {
            name: "apply_patch_v1",
            description: "Apply a validated patch_v1 atomically inside the workspace",
            category: ToolCategory::File,
            safety: ToolSafety::LocalMutating,
            confirmation_required: true,
        });

        // Real authorized test execution (P3): runs a kernel-derived
        // allowlisted test command inside the workspace (direct argv spawn,
        // NO shell, hard timeout) and returns a kernel-owned test_report_v1.
        // The command is never constructed from LLM output. Mutating (tests
        // may write inside the workspace), therefore confirmation-gated; the
        // kernel effect path supplies its own policy confirmation.
        self.register(ToolDef {
            name: "run_tests_v1",
            description: "Execute the allowlisted test command for a workspace (no shell)",
            category: ToolCategory::Shell,
            safety: ToolSafety::LocalMutating,
            confirmation_required: true,
        });

        // Shell tools. There is exactly one shell tool and it ALWAYS
        // requires confirmation. The former `shell_execute_readonly` alias
        // dispatched to the same `sh -c` implementation without
        // confirmation (audit findings H4/S3) and was removed: "readonly"
        // must be an enforced policy, not a bypassable name decoration.
        self.register(ToolDef {
            name: "shell_execute",
            description: "Execute a shell command",
            category: ToolCategory::Shell,
            safety: ToolSafety::ExternalMutating,
            confirmation_required: true,
        });

        // Web tools
        self.register(ToolDef {
            name: "fetch_url",
            description: "Fetch URL content",
            category: ToolCategory::Web,
            safety: ToolSafety::ReadOnly,
            confirmation_required: false,
        });
        self.register(ToolDef {
            name: "open_url",
            description: "Open URL in browser",
            category: ToolCategory::Web,
            safety: ToolSafety::ExternalMutating,
            confirmation_required: true,
        });
    }

    pub fn register(&mut self, tool: ToolDef) {
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.iter().find(|t| t.name == name)
    }

    pub fn all(&self) -> &[ToolDef] {
        &self.tools
    }

    pub fn by_category(&self, cat: ToolCategory) -> Vec<&ToolDef> {
        self.tools.iter().filter(|t| t.category == cat).collect()
    }

    pub fn requires_confirmation(&self, name: &str) -> bool {
        self.get(name)
            .map(|t| t.confirmation_required)
            .unwrap_or(true)
    }
}

/// Execute a tool by name with JSON arguments.
///
/// This is the central dispatch point and the ENFORCEMENT boundary for tool
/// authorization (audit findings H4/S3): every mutating tool requires an
/// explicit `confirmed = true` here, regardless of which UI or caller is in
/// front. Unknown tools fail closed. Callers cannot bypass the gate by
/// dispatching around this function — it is the only public executor.
pub async fn execute_tool(
    name: &str,
    args: &serde_json::Value,
    workspace: &str,
    confirmed: bool,
) -> ToolResult {
    let start = std::time::Instant::now();

    // Authorization gate (enforced, not advisory).
    let def = match registry_lookup(name) {
        Some(d) => d,
        None => {
            return ToolResult::err(format!("unknown tool: {}", name), 0);
        }
    };
    if def.confirmation_required && !confirmed {
        return ToolResult::err(
            format!(
                "authorization required: tool '{}' requires explicit confirmation",
                name
            ),
            0,
        );
    }

    let result = match name {
        "read_file" => super::file_tools::read_file(args, workspace).await,
        "write_file" => super::file_tools::write_file(args, workspace).await,
        "edit_file" => super::file_tools::edit_file(args, workspace).await,
        "list_directory" => super::file_tools::list_directory(args, workspace).await,
        "find_files" => super::file_tools::find_files(args, workspace).await,
        "grep_files" => super::file_tools::grep_files(args, workspace).await,
        "get_file_info" => super::file_tools::get_file_info(args, workspace).await,
        "apply_patch_v1" => super::patch_tool::apply_patch(args, workspace).await,
        "run_tests_v1" => super::test_runner::run_tests_tool(args, workspace).await,
        "shell_execute" => super::shell_tools::shell_execute(args, workspace).await,
        "fetch_url" => super::web_tools::fetch_url(args).await,
        "open_url" => super::web_tools::open_url(args).await,
        _ => Err(format!("Unknown tool: {}", name)),
    };
    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(output) => ToolResult::ok(output, duration_ms),
        Err(error) => ToolResult::err(error, duration_ms),
    }
}

/// Static metadata lookup used by the gate. Mirrors the registered defaults
/// so the authorization decision cannot be affected by registry mutation.
fn registry_lookup(name: &str) -> Option<ToolDef> {
    let registry = ToolRegistry::new();
    registry.get(name).cloned()
}
