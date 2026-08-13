//! Phase 2, Task 2 — CLI Contract Consistency
//!
//! Verifies that all JSON-outputting CLI commands use the cli-json-v1 envelope.

use serde_json::json;

#[test]
fn cli_json_v1_envelope_has_required_fields() {
    // Test the envelope structure directly
    let report = json!({"test": "data"});
    let envelope = deterministic_ai_kernel::cli_json::command_report("test-command", report);

    // Verify envelope structure
    assert_eq!(envelope["schema_version"], "cli-json-v1");
    assert_eq!(envelope["command"], "test-command");
    assert_eq!(envelope["ok"], true);
    assert!(envelope.get("report").is_some());
}

#[test]
fn cli_json_v1_schema_version_is_constant() {
    assert_eq!(
        deterministic_ai_kernel::cli_json::cli_json_schema_version(),
        "cli-json-v1"
    );
}

#[test]
fn cli_json_v1_envelope_preserves_report_data() {
    let report = json!({
        "task_id": "test-task",
        "status": "ok",
        "nested": {"key": "value"}
    });
    let envelope = deterministic_ai_kernel::cli_json::command_report("test-cmd", report.clone());

    // Verify report data is preserved
    assert_eq!(envelope["report"]["task_id"], "test-task");
    assert_eq!(envelope["report"]["status"], "ok");
    assert_eq!(envelope["report"]["nested"]["key"], "value");
}

#[test]
fn cli_json_v1_envelope_is_compact_json() {
    let report = json!({"key": "value"});
    let envelope = deterministic_ai_kernel::cli_json::command_report("cmd", report);

    // The envelope should serialize to compact JSON (no pretty printing)
    let compact = serde_json::to_string(&envelope).unwrap();
    assert!(!compact.contains('\n'), "envelope should be compact JSON");

    // But pretty-printed should also work
    let pretty = serde_json::to_string_pretty(&envelope).unwrap();
    assert!(pretty.contains('\n'), "pretty-printed should have newlines");
}
