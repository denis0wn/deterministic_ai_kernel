//! Operational envelope — the ONLY place wall-clock metadata lives.
//!
//! Byte-stable decision cores, work orders and chain cores never carry
//! timestamps; the envelope is attached at write time and excluded from
//! every content hash (same separation as `audit_log_v1.json` in v0.2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationalStamp {
    pub recorded_at_iso: String,
    pub ts_unix: u64,
}

/// Current-time stamp. Used only for operational envelopes, never for
/// content that participates in hashes.
pub fn now_stamp() -> OperationalStamp {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    OperationalStamp {
        recorded_at_iso: chrono::DateTime::from_timestamp(ts as i64, 0)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_else(|| ts.to_string()),
        ts_unix: ts,
    }
}
