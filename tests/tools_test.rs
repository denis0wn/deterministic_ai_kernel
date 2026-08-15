use deterministic_ai_kernel::tools::*;

#[test]
fn registry_has_all_tools() {
    let reg = ToolRegistry::new();
    let tools = reg.all();
    assert!(
        tools.len() >= 10,
        "Expected at least 10 tools, got {}",
        tools.len()
    );

    // File tools
    assert!(reg.get("read_file").is_some());
    assert!(reg.get("write_file").is_some());
    assert!(reg.get("edit_file").is_some());
    assert!(reg.get("list_directory").is_some());
    assert!(reg.get("find_files").is_some());
    assert!(reg.get("grep_files").is_some());
    assert!(reg.get("get_file_info").is_some());

    // Shell surface: the LLM never drives shell execution (security debt
    // closure). `shell_execute` (arbitrary `sh -c`) and its former readonly
    // alias must both be absent; the only Shell-category tool is the
    // kernel-owned, allowlisted `run_tests_v1`.
    assert!(reg.get("shell_execute").is_none());
    assert!(reg.get("shell_execute_readonly").is_none());

    // Web tools
    assert!(reg.get("fetch_url").is_some());
    assert!(reg.get("open_url").is_some());
}

#[test]
fn tool_safety_classification() {
    let reg = ToolRegistry::new();

    // Read-only tools should not require confirmation
    assert!(!reg.requires_confirmation("read_file"));
    assert!(!reg.requires_confirmation("list_directory"));
    assert!(!reg.requires_confirmation("find_files"));
    assert!(!reg.requires_confirmation("grep_files"));
    assert!(!reg.requires_confirmation("get_file_info"));

    // Mutating tools should require confirmation
    assert!(reg.requires_confirmation("write_file"));
    assert!(reg.requires_confirmation("edit_file"));
    assert!(reg.requires_confirmation("open_url"));

    // Unknown tools fail closed.
    assert!(reg.requires_confirmation("nonexistent_tool"));
}

#[test]
fn tool_categories() {
    let reg = ToolRegistry::new();

    let file_tools = reg.by_category(ToolCategory::File);
    assert!(file_tools.len() >= 7);

    // Shell category: only the kernel-owned `run_tests_v1` execution tool
    // (P3) remains. The interactive `shell_execute` was removed (security
    // debt closure): LLM-proposed shell text must never be executed.
    let shell_tools = reg.by_category(ToolCategory::Shell);
    assert_eq!(shell_tools.len(), 1);
    assert_eq!(shell_tools[0].name, "run_tests_v1");

    let web_tools = reg.by_category(ToolCategory::Web);
    assert!(web_tools.len() >= 2);
}

#[tokio::test]
async fn mutating_tools_require_confirmation() {
    // Without confirmation the gate rejects mutating tools before any
    // execution happens.
    let edit = execute_tool(
        "edit_file",
        &serde_json::json!({"path": "/tmp/dak_gate_edit.txt", "old": "a", "new": "b"}),
        "/tmp",
        false,
    )
    .await;
    assert!(!edit.success);
    assert!(edit.error.unwrap().contains("authorization required"));

    let write = execute_tool(
        "write_file",
        &serde_json::json!({"path": "/tmp/dak_gate_test.txt", "content": "x"}),
        "/tmp",
        false,
    )
    .await;
    assert!(!write.success);
    assert!(write.error.unwrap().contains("authorization required"));
    assert!(!std::path::Path::new("/tmp/dak_gate_test.txt").exists());

    // Read-only tools run without confirmation.
    let read = execute_tool(
        "read_file",
        &serde_json::json!({"path": "Cargo.toml"}),
        ".",
        false,
    )
    .await;
    assert!(read.success);
}

#[tokio::test]
async fn read_file_tool() {
    let result = execute_tool(
        "read_file",
        &serde_json::json!({"path": "Cargo.toml"}),
        ".",
        false,
    )
    .await;
    assert!(result.success);
    let content = result
        .output
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(content.contains("[package]"));
}

#[tokio::test]
async fn list_directory_tool() {
    let result = execute_tool(
        "list_directory",
        &serde_json::json!({"path": "src"}),
        ".",
        false,
    )
    .await;
    assert!(result.success);
    let count = result.output.get("count").and_then(|v| v.as_u64()).unwrap();
    assert!(count > 0);
}

#[tokio::test]
async fn write_file_and_read_back() {
    let test_path = "/tmp/dak_test_tool.txt";
    let write_result = execute_tool(
        "write_file",
        &serde_json::json!({
            "path": test_path,
            "content": "hello from tool test"
        }),
        "/tmp",
        true,
    )
    .await;
    assert!(write_result.success);

    let read_result = execute_tool(
        "read_file",
        &serde_json::json!({"path": test_path}),
        "/tmp",
        false,
    )
    .await;
    assert!(read_result.success);
    let content = read_result
        .output
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap();
    assert_eq!(content, "hello from tool test");

    // Cleanup
    let _ = std::fs::remove_file(test_path);
}

#[tokio::test]
async fn edit_file_tool() {
    let test_path = "/tmp/dak_test_edit.txt";
    std::fs::write(test_path, "hello world").unwrap();

    let result = execute_tool(
        "edit_file",
        &serde_json::json!({
            "path": test_path,
            "old_string": "world",
            "new_string": "Rust"
        }),
        "/tmp",
        true,
    )
    .await;
    assert!(result.success);

    let content = std::fs::read_to_string(test_path).unwrap();
    assert_eq!(content, "hello Rust");

    let _ = std::fs::remove_file(test_path);
}

#[tokio::test]
async fn shell_execute_is_gone_fail_closed() {
    // Security debt closure: `shell_execute` no longer exists, even with
    // confirmation. Unknown tools fail closed, so an LLM that still tries
    // to invoke it gets a hard rejection — no shell is ever spawned.
    let result = execute_tool(
        "shell_execute",
        &serde_json::json!({
            "command": "echo hello"
        }),
        ".",
        true,
    )
    .await;
    assert!(!result.success);
    assert!(
        result.error.unwrap().contains("unknown tool"),
        "shell_execute must be an unknown tool, not a live one"
    );
}

#[tokio::test]
async fn fetch_url_rejects_disallowed_targets() {
    // Non-http schemes are rejected.
    let file_url = execute_tool(
        "fetch_url",
        &serde_json::json!({"url": "file:///etc/passwd"}),
        ".",
        false,
    )
    .await;
    assert!(!file_url.success);
    assert!(file_url.error.unwrap().contains("not allowed"));

    // Loopback/private hosts are rejected (SSF mitigation, audit M5).
    let loopback = execute_tool(
        "fetch_url",
        &serde_json::json!({"url": "http://127.0.0.1:8080/"}),
        ".",
        false,
    )
    .await;
    assert!(!loopback.success);
    assert!(loopback.error.unwrap().contains("not allowed"));

    let metadata = execute_tool(
        "fetch_url",
        &serde_json::json!({"url": "http://169.254.169.254/latest/meta-data/"}),
        ".",
        false,
    )
    .await;
    assert!(!metadata.success);
    assert!(metadata.error.unwrap().contains("not allowed"));
}

#[tokio::test]
async fn find_files_tool() {
    let result = execute_tool(
        "find_files",
        &serde_json::json!({
            "pattern": "*.rs",
            "path": "src/tools"
        }),
        ".",
        false,
    )
    .await;
    assert!(result.success);
    let count = result.output.get("count").and_then(|v| v.as_u64()).unwrap();
    assert!(count >= 5, "Expected at least 5 .rs files in src/tools");
}

#[tokio::test]
async fn grep_files_tool() {
    let result = execute_tool(
        "grep_files",
        &serde_json::json!({
            "pattern": "pub fn",
            "path": "src/tools",
            "include": "rs"
        }),
        ".",
        false,
    )
    .await;
    assert!(result.success);
    let matches = result
        .output
        .get("matches")
        .and_then(|v| v.as_u64())
        .unwrap();
    assert!(matches > 0);
}

#[tokio::test]
async fn get_file_info_tool() {
    let result = execute_tool(
        "get_file_info",
        &serde_json::json!({"path": "Cargo.toml"}),
        ".",
        false,
    )
    .await;
    assert!(result.success);
    let is_file = result
        .output
        .get("is_file")
        .and_then(|v| v.as_bool())
        .unwrap();
    assert!(is_file);
}

#[tokio::test]
async fn path_escape_prevention() {
    let result = execute_tool(
        "read_file",
        &serde_json::json!({"path": "../etc/passwd"}),
        ".",
        false,
    )
    .await;
    assert!(
        !result.success,
        "Expected failure for path traversal, got success"
    );
    let err = result.error.unwrap();
    assert!(
        err.contains("escapes") || err.contains("read error") || err.contains("workspace"),
        "Expected escape/error message, got: {err}"
    );
}

#[test]
fn tool_result_envelope() {
    let ok = ToolResult::ok(serde_json::json!({"data": 42}), 100);
    assert!(ok.success);
    assert_eq!(ok.duration_ms, 100);

    let err = ToolResult::err("something broke".to_string(), 50);
    assert!(!err.success);
    assert!(err.error.is_some());
}

#[test]
fn confirmation_request_format() {
    let tool = ToolDef {
        name: "write_file",
        description: "test",
        category: ToolCategory::File,
        safety: ToolSafety::LocalMutating,
        confirmation_required: true,
    };
    let req = ConfirmationRequest::new(
        &tool,
        &serde_json::json!({"path": "/tmp/test.txt", "content": "data"}),
    );
    assert!(req.description.contains("Write"));
    assert!(req.description.contains("/tmp/test.txt"));
}
