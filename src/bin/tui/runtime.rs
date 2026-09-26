use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::app::StreamEvent;

/// Minimum interval between live probe calls.
const PROBE_THROTTLE_INTERVAL: Duration = Duration::from_secs(2);

/// Grace period after a failed probe: keep last known good state.
const PROBE_FAILURE_GRACE: Duration = Duration::from_secs(5);

/// How long to wait for server to come back after restart attempt.
pub const RECOVERY_WAIT_SECS: u64 = 15;

/// Maximum number of recovery attempts before giving up.
const MAX_RECOVERY_ATTEMPTS: u32 = 3;

/// Cooldown between recovery attempts.
const RECOVERY_COOLDOWN: Duration = Duration::from_secs(10);

// ── Server state machine ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    /// Initial state before first probe.
    Unknown,
    /// Probe succeeded — server is reachable.
    Online,
    /// Probe failed but no recent crash detected.
    Offline,
    /// Server was previously online, now failing — likely crashed.
    Crashed,
    /// Recovery in progress (restart launched, waiting for probe).
    Recovering,
}

impl ServerState {
    pub fn label(self) -> &'static str {
        match self {
            ServerState::Unknown => "unknown",
            ServerState::Online => "online",
            ServerState::Offline => "offline",
            ServerState::Crashed => "crashed",
            ServerState::Recovering => "recovering",
        }
    }

    pub fn is_usable(self) -> bool {
        self == ServerState::Online
    }
}

// ── Server ownership ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerOwnership {
    /// Server was launched by Replay OS — we can restart it.
    Managed,
    /// Server was launched externally — show hints, don't kill it.
    External,
    /// Ownership unknown — safe default: prompt before restart.
    Unknown,
}

impl ServerOwnership {
    pub fn label(self) -> &'static str {
        match self {
            ServerOwnership::Managed => "managed",
            ServerOwnership::External => "external",
            ServerOwnership::Unknown => "unknown",
        }
    }
}

// ── RuntimeState ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RuntimeState {
    pub is_running: bool,
    pub model_id: String,
    pub base_url: String,
    // Recovery fields
    pub server_state: ServerState,
    pub server_ownership: ServerOwnership,
    pub last_probe_at: Option<Instant>,
    pub last_success_at: Option<Instant>,
    pub last_failure_at: Option<Instant>,
    pub last_recovery_at: Option<Instant>,
    pub recovery_attempts: u32,
    pub last_error_reason: String,
    /// PID of server if managed by us.
    pub managed_server_pid: Option<u32>,
}

/// Statistics for probe call tracking.
#[derive(Debug, Default)]
pub struct ProbeStats {
    pub total_probes: u64,
    pub cache_hits: u64,
    pub live_probes: u64,
    pub failures: u64,
}

impl RuntimeState {
    /// Create initial state (no probe yet).
    pub fn initial() -> Self {
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_string());
        Self {
            is_running: false,
            model_id: "unknown".to_string(),
            base_url,
            server_state: ServerState::Unknown,
            server_ownership: ServerOwnership::Unknown,
            last_probe_at: None,
            last_success_at: None,
            last_failure_at: None,
            last_recovery_at: None,
            recovery_attempts: 0,
            last_error_reason: String::new(),
            managed_server_pid: None,
        }
    }

    /// Live probe — spawns curl subprocess. Only call when cache is stale.
    fn probe_live() -> (Self, Option<String>) {
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_string());

        let output = Command::new("curl")
            .args([
                "-s",
                "--max-time",
                "2",
                &format!("{}/models", base_url.trim_end_matches('/')),
            ])
            .output();

        let now = Instant::now();
        let (is_running, model_id, error_reason) = match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                if stdout.contains("\"data\"") {
                    let model_id = serde_json::from_str::<serde_json::Value>(&stdout)
                        .ok()
                        .and_then(|v| v["data"][0]["id"].as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "unknown".to_string());
                    (true, model_id, String::new())
                } else if stdout.is_empty() {
                    (false, "unknown".to_string(), "server_offline".to_string())
                } else {
                    (
                        false,
                        "unknown".to_string(),
                        format!("unexpected_response: {}", &stdout[..stdout.len().min(100)]),
                    )
                }
            }
            Err(e) => (false, "unknown".to_string(), format!("probe_error: {e}")),
        };

        let state = Self {
            is_running,
            model_id,
            base_url,
            server_state: ServerState::Unknown, // caller will update
            server_ownership: ServerOwnership::Unknown,
            last_probe_at: Some(now),
            last_success_at: if is_running { Some(now) } else { None },
            last_failure_at: if !is_running { Some(now) } else { None },
            last_recovery_at: None,
            recovery_attempts: 0,
            last_error_reason: error_reason.clone(),
            managed_server_pid: None,
        };

        (
            state,
            if error_reason.is_empty() {
                None
            } else {
                Some(error_reason)
            },
        )
    }

    /// Determine server state transition after a failed probe.
    fn compute_server_state(&self, was_previously_online: bool) -> ServerState {
        if was_previously_online {
            // Was online, now failing → crash
            ServerState::Crashed
        } else if self.server_state == ServerState::Recovering {
            // Was recovering, still failing
            ServerState::Crashed
        } else {
            ServerState::Offline
        }
    }

    /// Cached probe — returns cached state if fresh, spawns live probe if stale.
    /// Updates stats counters.
    pub fn probe_cached(&mut self, stats: &mut ProbeStats) {
        stats.total_probes += 1;

        let cache_fresh = self
            .last_probe_at
            .map(|t| t.elapsed() < PROBE_THROTTLE_INTERVAL)
            .unwrap_or(false);

        if cache_fresh {
            stats.cache_hits += 1;
            return;
        }

        // Cache is stale — do a live probe
        stats.live_probes += 1;
        let was_previously_online = self.is_running;
        let was_recovering = self.server_state == ServerState::Recovering;
        let (new_state, error_reason) = Self::probe_live();

        if new_state.is_running {
            self.is_running = true;
            self.model_id = new_state.model_id;
            self.server_state = ServerState::Online;
            self.last_probe_at = new_state.last_probe_at;
            self.last_success_at = new_state.last_success_at;
            self.last_failure_at = None;
            self.last_error_reason.clear();
            // If we were recovering and now online, reset recovery attempts
            if was_recovering {
                self.recovery_attempts = 0;
            }
        } else {
            // Probe failed
            self.last_probe_at = new_state.last_probe_at;
            self.last_failure_at = new_state.last_failure_at;
            if let Some(reason) = error_reason {
                self.last_error_reason = reason;
            }

            // Use grace period with last known good state
            let last_good_age = self
                .last_success_at
                .map(|t| t.elapsed())
                .unwrap_or(Duration::from_secs(999));

            if last_good_age < PROBE_FAILURE_GRACE {
                // Within grace period: keep last known good state
                stats.failures += 1;
            } else {
                // Grace expired: accept the failure
                self.is_running = false;
                self.model_id = "unknown".to_string();
                self.last_success_at = None;
                stats.failures += 1;
            }

            // Compute new server state
            self.server_state = self.compute_server_state(was_previously_online);
        }
    }

    /// Legacy probe — for one-shot checks outside tick loop.
    pub fn probe() -> Self {
        let (state, _) = Self::probe_live();
        state
    }

    /// Force a live probe and update self in place.
    pub fn probe_force(&mut self, stats: &mut ProbeStats) {
        stats.total_probes += 1;
        stats.live_probes += 1;
        let was_previously_online = self.is_running;
        let (new_state, error_reason) = Self::probe_live();
        self.is_running = new_state.is_running;
        self.model_id = new_state.model_id;
        self.last_probe_at = new_state.last_probe_at;
        self.last_success_at = new_state.last_success_at;
        if let Some(reason) = error_reason {
            self.last_error_reason = reason;
        }
        if new_state.is_running {
            self.server_state = ServerState::Online;
            self.last_failure_at = None;
            self.last_error_reason.clear();
        } else {
            stats.failures += 1;
            self.last_failure_at = new_state.last_failure_at;
            self.server_state = self.compute_server_state(was_previously_online);
        }
    }

    /// Check if recovery is allowed (rate-limited).
    pub fn can_recover(&self) -> bool {
        if self.recovery_attempts >= MAX_RECOVERY_ATTEMPTS {
            return false;
        }
        if let Some(last) = self.last_recovery_at {
            if last.elapsed() < RECOVERY_COOLDOWN {
                return false;
            }
        }
        true
    }

    /// Start recovery: launch mlx_lm.server if managed, or return instructions.
    pub fn start_recovery(&mut self) -> RecoveryAction {
        if !self.can_recover() {
            return RecoveryAction::RateLimited {
                message: format!(
                    "Recovery rate-limited. {} attempt(s) used. Wait {}s or restart manually.",
                    self.recovery_attempts,
                    RECOVERY_COOLDOWN.as_secs()
                ),
            };
        }

        self.recovery_attempts += 1;
        self.last_recovery_at = Some(Instant::now());
        self.server_state = ServerState::Recovering;

        match self.server_ownership {
            ServerOwnership::Managed => {
                // Try to restart the server
                let model_path = self.resolve_model_path();
                let port = self.extract_port();

                let pid = Command::new("mlx_lm.server")
                    .args([
                        "--model", &model_path,
                        "--port", &port.to_string(),
                        "--decode-concurrency", "1",
                        "--prompt-concurrency", "1",
                    ])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .ok()
                    .map(|c| c.id());

                if let Some(pid) = pid {
                    self.managed_server_pid = Some(pid);
                    RecoveryAction::Launched {
                        message: format!("Server launched (pid={pid}). Waiting for readiness..."),
                        pid,
                    }
                } else {
                    self.server_state = ServerState::Offline;
                    RecoveryAction::LaunchFailed {
                        message: "Failed to launch mlx_lm.server. Check if mlx_lm is installed.".to_string(),
                    }
                }
            }
            ServerOwnership::External | ServerOwnership::Unknown => {
                RecoveryAction::ShowInstructions {
                    message: format!(
                        "Run manually:\n  mlx_lm.server --model {} --port {} --decode-concurrency 1 --prompt-concurrency 1",
                        self.resolve_model_path(),
                        self.extract_port()
                    ),
                }
            }
        }
    }

    /// Check if managed server process is still alive.
    pub fn check_managed_server_alive(&self) -> bool {
        if let Some(pid) = self.managed_server_pid {
            #[cfg(unix)]
            {
                unsafe { libc::kill(pid as i32, 0) == 0 }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                true
            }
        } else {
            false
        }
    }

    /// Reset recovery state (e.g., after user dismisses recovery UI).
    pub fn reset_recovery(&mut self) {
        self.recovery_attempts = 0;
        self.last_recovery_at = None;
    }

    /// Resolve model path from env or manifest.
    fn resolve_model_path(&self) -> String {
        // Try manifest first via lib crate, fall back to env, then hardcoded default
        if let Ok(manifest) = deterministic_ai_kernel::model_manifest::load_manifest() {
            if let Some(model) = manifest
                .models
                .iter()
                .find(|m| m.role == "coding_assistant" && m.enabled)
            {
                return model.id.clone();
            }
        }
        std::env::var("OPENAI_MODEL")
            .unwrap_or_else(|_| "/Users/denissmoliakov/Models/ministral-14b-reasoning".to_string())
    }

    /// Extract port from base_url.
    fn extract_port(&self) -> u16 {
        self.base_url
            .split(':')
            .nth(1)
            .and_then(|s| s.split('/').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080)
    }
}

// ── Recovery types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum RecoveryAction {
    Launched { message: String, pid: u32 },
    LaunchFailed { message: String },
    ShowInstructions { message: String },
    RateLimited { message: String },
}

// ── Spawn functions (unchanged) ─────────────────────────────────────────────

/// Spawn a command and stream its output through a channel.
pub fn spawn_streaming(cmd: &str, args: &[&str], tx: mpsc::Sender<StreamEvent>) -> Option<u32> {
    let project_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut child = Command::new(cmd)
        .args(args)
        .current_dir(project_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let child_id = child.id();
    let _ = tx.send(StreamEvent::Started { child_id });

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    if let Some(stdout) = stdout {
        let tx_clone = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        let _ = tx_clone.send(StreamEvent::Output(l));
                    }
                    Err(_) => break,
                }
            }
        });
    }

    if let Some(stderr) = stderr {
        let tx_clone = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        let _ = tx_clone.send(StreamEvent::Error(l));
                    }
                    Err(_) => break,
                }
            }
        });
    }

    std::thread::spawn(move || {
        let status = child.wait();
        let _ = tx.send(StreamEvent::Finished {
            success: status.map(|s| s.success()).unwrap_or(false),
        });
    });

    Some(child_id)
}

pub fn spawn_verified_execution(
    payload: &str,
    seed: u64,
    tx: mpsc::Sender<StreamEvent>,
) -> Option<u32> {
    let project_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let run_bin = project_dir.join("target/debug/run");
    spawn_streaming(
        run_bin.to_str().unwrap_or("target/debug/run"),
        &[
            "execute",
            "--payload",
            payload,
            "--seed",
            &seed.to_string(),
            "--json",
        ],
        tx,
    )
}

pub fn spawn_plan_only(payload: &str, seed: u64, tx: mpsc::Sender<StreamEvent>) -> Option<u32> {
    spawn_streaming(
        "cargo",
        &[
            "run",
            "--bin",
            "deterministic_ai_kernel",
            "--",
            "pipeline-run",
            "--payload",
            payload,
            "--seed",
            &seed.to_string(),
            "--json",
        ],
        tx,
    )
}

pub fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_state_labels() {
        assert_eq!(ServerState::Online.label(), "online");
        assert_eq!(ServerState::Offline.label(), "offline");
        assert_eq!(ServerState::Crashed.label(), "crashed");
        assert_eq!(ServerState::Recovering.label(), "recovering");
        assert_eq!(ServerState::Unknown.label(), "unknown");
    }

    #[test]
    fn server_state_usable() {
        assert!(ServerState::Online.is_usable());
        assert!(!ServerState::Offline.is_usable());
        assert!(!ServerState::Crashed.is_usable());
        assert!(!ServerState::Recovering.is_usable());
        assert!(!ServerState::Unknown.is_usable());
    }

    #[test]
    fn ownership_labels() {
        assert_eq!(ServerOwnership::Managed.label(), "managed");
        assert_eq!(ServerOwnership::External.label(), "external");
        assert_eq!(ServerOwnership::Unknown.label(), "unknown");
    }

    #[test]
    fn compute_state_transitions() {
        let state = RuntimeState::initial();
        // Online → probe fail → Crashed
        assert_eq!(state.compute_server_state(true), ServerState::Crashed);
        // Offline → probe fail → Offline
        assert_eq!(state.compute_server_state(false), ServerState::Offline);
    }

    #[test]
    fn can_recover_initially() {
        let state = RuntimeState::initial();
        assert!(state.can_recover());
    }

    #[test]
    fn can_recover_rate_limited() {
        let mut state = RuntimeState::initial();
        state.recovery_attempts = MAX_RECOVERY_ATTEMPTS;
        assert!(!state.can_recover());
    }

    #[test]
    fn recovery_cooldown() {
        let mut state = RuntimeState::initial();
        state.last_recovery_at = Some(Instant::now());
        assert!(!state.can_recover());
    }

    #[test]
    fn reset_recovery_clears_state() {
        let mut state = RuntimeState::initial();
        state.recovery_attempts = 5;
        state.last_recovery_at = Some(Instant::now());
        state.reset_recovery();
        assert_eq!(state.recovery_attempts, 0);
        assert!(state.last_recovery_at.is_none());
    }

    #[test]
    fn managed_server_pid_check() {
        let mut state = RuntimeState::initial();
        state.managed_server_pid = Some(999999); // non-existent PID
        assert!(!state.check_managed_server_alive());
    }

    #[test]
    fn extract_port_from_url() {
        let mut state = RuntimeState::initial();
        state.base_url = "http://127.0.0.1:8080/v1".to_string();
        assert_eq!(state.extract_port(), 8080);
    }

    #[test]
    fn recovery_action_rate_limited() {
        let mut state = RuntimeState::initial();
        state.recovery_attempts = MAX_RECOVERY_ATTEMPTS;
        let action = state.start_recovery();
        assert!(matches!(action, RecoveryAction::RateLimited { .. }));
    }
}
