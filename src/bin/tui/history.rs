use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use deterministic_ai_kernel::tools::ToolInvocation;

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub timestamp: String,
    pub task: String,
    pub seed: String,
    pub ok: bool,
    pub plan_id: String,
    pub answer: String,
    pub elapsed_ms: String,
    pub file_path: PathBuf,
    pub entry_type: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub confirmed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HistoryFile {
    timestamp: String,
    task: String,
    seed: String,
    ok: String,
    plan_id: String,
    answer: String,
    elapsed_ms: String,
}

fn history_dir() -> PathBuf {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("config")
        .join("model_manifest.json");
    let project_dir = manifest_path.parent().unwrap().parent().unwrap();
    project_dir.join(".replay_os").join("history")
}

pub fn load_history() -> Vec<HistoryEntry> {
    let dir = history_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let mut entries: Vec<HistoryEntry> = fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let content = fs::read_to_string(e.path()).ok()?;
            let raw: serde_json::Value = serde_json::from_str(&content).ok()?;

            // Detect entry type
            let entry_type = raw
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("task_run");

            if entry_type == "tool_invocation" {
                let tool_name = raw
                    .get("tool_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let status = raw
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string();
                let confirmed = raw
                    .get("confirmed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let duration_ms = raw.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                let error = raw
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let output_summary = raw
                    .get("output_summary")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let arguments = raw
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                Some(HistoryEntry {
                    timestamp: raw
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    task: format!("[tool] {} → {}", tool_name, status),
                    seed: String::new(),
                    ok: status == "Success",
                    plan_id: String::new(),
                    answer: output_summary.unwrap_or_default(),
                    elapsed_ms: duration_ms.to_string(),
                    file_path: e.path(),
                    entry_type: "tool_invocation".to_string(),
                    tool_name,
                    arguments,
                    confirmed,
                    error,
                })
            } else {
                let file: HistoryFile = serde_json::from_str(&content).ok()?;
                Some(HistoryEntry {
                    timestamp: file.timestamp,
                    task: file.task,
                    seed: file.seed,
                    ok: file.ok == "true",
                    plan_id: file.plan_id,
                    answer: file.answer,
                    elapsed_ms: file.elapsed_ms,
                    file_path: e.path(),
                    entry_type: "task_run".to_string(),
                    tool_name: String::new(),
                    arguments: serde_json::Value::Null,
                    confirmed: false,
                    error: None,
                })
            }
        })
        .collect();

    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    entries
}

pub fn save_entry(task: &str, seed: u64, json: &serde_json::Value) {
    let dir = history_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }

    let ts = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let filename = format!("{}_seed{}.json", ts, seed);
    let path = dir.join(filename);

    let ok = json.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let plan_id = json
        .get("report")
        .and_then(|r| r.get("plan_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("n/a");
    let answer = json
        .get("report")
        .and_then(|r| r.get("final_answer"))
        .and_then(|v| v.as_str())
        .unwrap_or("n/a");
    let elapsed = json
        .get("report")
        .and_then(|r| r.get("elapsed_ms"))
        .and_then(|v| v.as_u64())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "n/a".to_string());

    let entry = serde_json::json!({
        "timestamp": ts,
        "task": task,
        "seed": seed.to_string(),
        "ok": ok.to_string(),
        "plan_id": plan_id,
        "answer": answer,
        "elapsed_ms": elapsed,
        "raw": json,
    });

    let _ = fs::write(path, serde_json::to_string_pretty(&entry).unwrap());
}

pub fn save_tool_invocation(invocation: &ToolInvocation) {
    let dir = history_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }

    let ts = &invocation.timestamp;
    let filename = format!("tool_{}_{}.json", invocation.tool_name, ts.replace(':', ""));
    let path = dir.join(filename);

    let entry = serde_json::json!({
        "type": "tool_invocation",
        "timestamp": invocation.timestamp,
        "tool_name": invocation.tool_name,
        "arguments": invocation.arguments,
        "status": format!("{:?}", invocation.status),
        "output_summary": invocation.output.as_ref().map(|o| {
            if o.len() > 200 { format!("{}...", &o[..200]) } else { o.clone() }
        }),
        "error": invocation.error,
        "duration_ms": invocation.duration_ms,
        "confirmed": invocation.confirmed,
    });

    let _ = fs::write(path, serde_json::to_string_pretty(&entry).unwrap());
}

pub fn delete_entry(path: &PathBuf) -> std::io::Result<()> {
    fs::remove_file(path)
}
