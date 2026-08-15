use super::contract::ToolDef;

/// A pending confirmation request.
#[derive(Debug, Clone)]
pub struct ConfirmationRequest {
    pub tool: String,
    pub arguments: serde_json::Value,
    pub description: String,
}

impl ConfirmationRequest {
    pub fn new(tool: &ToolDef, args: &serde_json::Value) -> Self {
        let description = format_description(tool.name, args);
        Self {
            tool: tool.name.to_string(),
            arguments: args.clone(),
            description,
        }
    }
}

fn format_description(tool_name: &str, args: &serde_json::Value) -> String {
    match tool_name {
        "write_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let bytes = args
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.len())
                .unwrap_or(0);
            format!("Write {} bytes to {}", bytes, path)
        }
        "edit_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            format!("Edit file {}", path)
        }
        "open_url" => {
            let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("?");
            format!("Open URL: {}", url)
        }
        _ => format!("Run tool: {}", tool_name),
    }
}
