use serde_json::Value;
use std::process::Command;

pub trait CliJsonInput {
    fn to_value(&self) -> Value;
}

impl CliJsonInput for str {
    fn to_value(&self) -> Value {
        serde_json::from_str(self).expect("stdout must be valid json")
    }
}

impl CliJsonInput for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
}

pub fn assert_cli_json_v1<T: CliJsonInput + ?Sized>(input: &T, expected_command: &str) -> Value {
    let value = input.to_value();
    let obj = value.as_object().expect("top-level json must be an object");

    assert_eq!(
        obj.get("command").and_then(|v| v.as_str()),
        Some(expected_command)
    );
    assert_eq!(
        obj.get("schema_version").and_then(|v| v.as_str()),
        Some("cli-json-v1")
    );
    assert!(obj.contains_key("ok"), "top-level json must contain ok");

    value
}

fn unique_db(label: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir()
        .join(format!("dak_cli_json_{}_{}.db", label, nanos))
        .display()
        .to_string()
}

#[test]
fn integrity_json_emits_valid_cli_json_contract() {
    let bin = env!("CARGO_BIN_EXE_deterministic_ai_kernel");
    let db = unique_db("integrity");

    let output = Command::new(bin)
        .arg("integrity-json")
        .env("KERNEL_DB_PATH", &db)
        .output()
        .expect("failed to run integrity-json");

    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(format!("{db}-wal"));
    let _ = std::fs::remove_file(format!("{db}-shm"));

    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be utf8");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout must be valid json");

    let obj = value.as_object().expect("top-level json must be an object");

    assert_eq!(
        obj.get("command").and_then(|v| v.as_str()),
        Some("integrity-json")
    );
    assert_eq!(obj.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        obj.get("schema_version").and_then(|v| v.as_str()),
        Some("cli-json-v1")
    );

    let report = obj
        .get("report")
        .and_then(|v| v.as_object())
        .expect("report must be an object");

    assert_eq!(report.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        report.get("schema_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        report.get("snapshot_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        report.get("task_id").and_then(|v| v.as_str()),
        Some("integrity-task")
    );
    assert_eq!(
        report.get("created_at_present").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        report.get("state_present").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        report.get("state_hash_present").and_then(|v| v.as_bool()),
        Some(true)
    );
}
