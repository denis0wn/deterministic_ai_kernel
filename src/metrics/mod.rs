//! Lightweight deterministic runtime metrics collector.
//!
//! - Zero external dependencies (only std).
//! - Thread-safe via `Mutex`.
//! - JSON-serializable via manual `serde_json::Value` construction.
//! - Resettable for tests.
//!
//! # Usage
//! ```rust,ignore
//! use deterministic_ai_kernel::metrics::METRICS;
//! let _guard = METRICS.record("sqlite_read_ms", 3);
//! let snapshot = METRICS.snapshot();
//! ```

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

// ── Counter store ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct MetricEntry {
    total_ms: u64,
    count: u64,
    last_ms: u64,
}

#[derive(Debug, Default)]
pub struct MetricsCollector {
    entries: Mutex<BTreeMap<String, MetricEntry>>,
}

impl MetricsCollector {
    pub const fn new() -> Self {
        // Mutex::new is not const in stable Rust for BTreeMap, so we use
        // a wrapper that lazily initialises. The static is accessed through
        // METRICS which is a LazyLock / once_cell equivalent.
        // We use `unsafe` const-init trick via MaybeUninit-free approach:
        // simply declare and the Default impl handles it when first accessed.
        MetricsCollector {
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    /// Record a single timing observation (in milliseconds).
    pub fn record(&self, key: &str, ms: u64) {
        if let Ok(mut map) = self.entries.lock() {
            let e = map.entry(key.to_string()).or_default();
            e.total_ms += ms;
            e.count += 1;
            e.last_ms = ms;
        }
    }

    /// Reset all metrics (for test isolation).
    pub fn reset(&self) {
        if let Ok(mut map) = self.entries.lock() {
            map.clear();
        }
    }

    /// Snapshot all metrics as a JSON-serializable `serde_json::Value`.
    pub fn snapshot(&self) -> serde_json::Value {
        let map = match self.entries.lock() {
            Ok(m) => m,
            Err(_) => return serde_json::Value::Null,
        };
        let mut out = serde_json::Map::new();
        for (key, entry) in map.iter() {
            let avg = if entry.count > 0 {
                entry.total_ms as f64 / entry.count as f64
            } else {
                0.0
            };
            out.insert(
                key.clone(),
                serde_json::json!({
                    "total_ms":  entry.total_ms,
                    "count":     entry.count,
                    "avg_ms":    avg,
                    "last_ms":   entry.last_ms,
                }),
            );
        }
        serde_json::Value::Object(out)
    }

    /// Returns average latency for a given key, or 0.0.
    pub fn avg_ms(&self, key: &str) -> f64 {
        if let Ok(map) = self.entries.lock() {
            if let Some(e) = map.get(key) {
                if e.count > 0 {
                    return e.total_ms as f64 / e.count as f64;
                }
            }
        }
        0.0
    }

    /// Returns operation count for a given key, or 0.
    pub fn count(&self, key: &str) -> u64 {
        if let Ok(map) = self.entries.lock() {
            map.get(key).map(|e| e.count).unwrap_or(0)
        } else {
            0
        }
    }

    /// Returns last latency observation for a given key, or 0.
    pub fn last_ms(&self, key: &str) -> u64 {
        if let Ok(map) = self.entries.lock() {
            map.get(key).map(|e| e.last_ms).unwrap_or(0)
        } else {
            0
        }
    }
}

// ── Global singleton ──────────────────────────────────────────────────────────

/// Process-global metrics collector.
pub static METRICS: MetricsCollector = MetricsCollector {
    entries: Mutex::new(BTreeMap::new()),
};

// ── Timing guard ──────────────────────────────────────────────────────────────

/// RAII guard that records elapsed time in milliseconds when dropped.
pub struct TimingGuard {
    key: &'static str,
    start: Instant,
}

impl TimingGuard {
    pub fn start(key: &'static str) -> Self {
        Self {
            key,
            start: Instant::now(),
        }
    }
}

impl Drop for TimingGuard {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_millis() as u64;
        METRICS.record(self.key, elapsed);
    }
}

/// Convenience macro — times a block and records under the given key.
#[macro_export]
macro_rules! timed {
    ($key:literal, $block:expr) => {{
        let _guard = $crate::metrics::TimingGuard::start($key);
        $block
    }};
}

// ── Metric key constants ──────────────────────────────────────────────────────

pub const FINGERPRINT_GENERATION_MS: &str = "fingerprint_generation_ms";
pub const ARTIFACT_LOOKUP_MS: &str = "artifact_lookup_ms";
pub const PRIMITIVE_CACHE_LOOKUP_MS: &str = "primitive_cache_lookup_ms";
pub const PLANNER_CACHE_LOOKUP_MS: &str = "planner_cache_lookup_ms";
pub const SQLITE_READ_MS: &str = "sqlite_read_ms";
pub const SQLITE_WRITE_MS: &str = "sqlite_write_ms";
pub const PRIMITIVE_EXECUTION_MS: &str = "primitive_execution_ms";
pub const LLM_LATENCY_MS: &str = "llm_latency_ms";
