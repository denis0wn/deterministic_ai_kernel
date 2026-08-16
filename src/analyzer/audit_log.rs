//! Analyzer audit-trail — every analyzer run leaves a machine-readable
//! journal so the analyzer's own actions are auditable (mirrors the
//! kernel's evidence philosophy at the analysis layer).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub ts_unix: u64,
    pub workspace: String,
    pub files_scanned: usize,
    pub candidates_found: usize,
    pub findings_count: usize,
    pub tasks_emitted: Vec<String>,
    pub analyzer_version: String,
    pub duration_ms: u64,
}

/// Write `record` to `<dir>/run_<run_id>.json`. Returns the path.
pub fn write_run_record(dir: &Path, record: &RunRecord) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("run_{}.json", record.run_id));
    let json = serde_json::to_string_pretty(record).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// Deterministic run id from the workspace root + timestamp: readable,
/// sortable, collision-resistant for human review.
pub fn make_run_id(workspace: &str, ts_unix: u64) -> String {
    let stem = Path::new(workspace)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("workspace");
    format!("{stem}_{ts_unix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_record_roundtrips_to_disk() {
        let dir = std::env::temp_dir().join(format!(
            "dak_analyzer_audit_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let rec = RunRecord {
            run_id: make_run_record_id(),
            ts_unix: 12345,
            workspace: "/tmp/ws".to_string(),
            files_scanned: 3,
            candidates_found: 3,
            findings_count: 3,
            tasks_emitted: vec!["T-1".to_string()],
            analyzer_version: "0.1.0".to_string(),
            duration_ms: 42,
        };
        let path = write_run_record(&dir, &rec).unwrap();
        let back: RunRecord =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back, rec);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn make_run_record_id() -> String {
        make_run_id("/tmp/ws", 12345)
    }
}
