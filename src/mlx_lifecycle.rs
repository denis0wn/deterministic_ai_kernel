//! Canonical MLX model lifecycle manager (P0 MLX lifecycle fix).
//!
//! Before this module existed, `mlx_lm.server` was spawned (manually or by
//! TUI recovery) and never stopped: `lm_control::load_model/unload_model`
//! explicitly returned "not supported in MLX architecture", and the server
//! itself has no idle timeout. The model therefore occupied unified memory
//! for hours after the last inference.
//!
//! This module is the single lifecycle owner for kernel-managed servers:
//!
//! ```text
//! IDLE -> MODEL_START -> MODEL_LOADED -> INFERENCE -> IDLE
//!      -> IDLE_TIMEOUT -> GRACEFUL_SHUTDOWN -> MODEL_UNLOADED
//! ```
//!
//! Rules:
//! - idle timeout counts from the LAST COMPLETED inference (default 120 s,
//!   `MLX_IDLE_TIMEOUT_SECS`; 0 disables automatic unload);
//! - a request before expiry resets the timer and reuses the running
//!   server; a request after unload restarts the server, waits for
//!   readiness, then proceeds;
//! - only servers THIS kernel started are ever stopped; a server already
//!   running at first contact is marked EXTERNAL and never touched;
//! - shutdown is graceful (SIGTERM, bounded wait, verified process+endpoint
//!   disappearance) with a bounded SIGKILL escalation only for stuck
//!   processes; success is never declared without verification;
//! - start/stop serialize against inference through one mutex; the watcher
//!   never stops a server with in-flight requests.
//!
//! Cross-process knowledge lives in a small JSON state file so short-lived
//! CLI processes can still honor the idle contract.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 120;
const DEFAULT_STARTUP_TIMEOUT_SECS: u64 = 300;
const DEFAULT_SHUTDOWN_GRACE_SECS: u64 = 20;
const READINESS_POLL: Duration = Duration::from_millis(500);
const WATCHER_TICK: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LifecycleState {
    Idle,
    ModelStart,
    ModelLoaded,
    ShuttingDown,
    Unloaded,
}

#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// 0 disables automatic unload.
    pub idle_timeout: Duration,
    pub startup_timeout: Duration,
    pub shutdown_grace: Duration,
    /// Master switch (`MLX_LIFECYCLE=off` disables management entirely).
    pub enabled: bool,
    /// Server executable (`MLX_SERVER_COMMAND`, test/operator hook).
    pub server_command: String,
    pub state_file: PathBuf,
}

impl LifecycleConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self::parse(&|key| std::env::var(key).ok())
    }

    /// Pure configuration parsing over an injectable lookup. `from_env`
    /// supplies the real process environment; tests supply deterministic
    /// fake lookups so they NEVER mutate the process-global env (mutating
    /// `MLX_LIFECYCLE` in one test would enable/disable lifecycle for every
    /// other concurrently running test in the same binary).
    pub fn parse(lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let parse_u64 = |key: &str, default: u64| -> u64 {
            lookup(key)
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(default)
        };
        let idle = parse_u64("MLX_IDLE_TIMEOUT_SECS", DEFAULT_IDLE_TIMEOUT_SECS);
        let startup = parse_u64("MLX_STARTUP_TIMEOUT_SECS", DEFAULT_STARTUP_TIMEOUT_SECS);
        let grace = parse_u64("MLX_SHUTDOWN_GRACE_SECS", DEFAULT_SHUTDOWN_GRACE_SECS);
        let enabled = lookup("MLX_LIFECYCLE")
            .map(|v| !matches!(v.trim().to_lowercase().as_str(), "off" | "0" | "disabled"))
            .unwrap_or(true);
        let server_command = lookup("MLX_SERVER_COMMAND")
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "mlx_lm.server".to_string());
        let state_file = lookup("DAK_MLX_LIFECYCLE_STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join("deterministic_ai_kernel_mlx_lifecycle.json")
            });
        LifecycleConfig {
            idle_timeout: Duration::from_secs(idle),
            startup_timeout: Duration::from_secs(startup),
            shutdown_grace: Duration::from_secs(grace),
            enabled,
            server_command,
            state_file,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PersistedState {
    pub pid: Option<u32>,
    pub owned: bool,
    pub state: LifecycleState,
    pub model: String,
    pub base_url: String,
    pub started_unix: u64,
    pub last_activity_unix: u64,
    /// P4-C: detached idle supervisor responsible for unloading the server
    /// after ephemeral CLI processes exit. `#[serde(default)]` keeps old
    /// state files readable.
    #[serde(default)]
    pub supervisor_pid: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ShutdownReport {
    pub attempted: bool,
    pub pid: Option<u32>,
    pub escalated_to_kill: bool,
    pub process_gone: bool,
    pub endpoint_down: bool,
    pub detail: String,
}

impl ShutdownReport {
    pub fn verified(&self) -> bool {
        self.attempted && self.process_gone && self.endpoint_down
    }
}

#[derive(Debug, Clone)]
pub struct LifecycleStatus {
    pub state: LifecycleState,
    pub enabled: bool,
    pub pid: Option<u32>,
    pub owned: bool,
    pub external: bool,
    pub endpoint_up: bool,
    pub idle_secs: u64,
    pub timeout_secs: u64,
    pub in_flight: usize,
    pub last_failure: Option<String>,
}

struct Inner {
    cfg: LifecycleConfig,
    state: LifecycleState,
    child: Option<Child>,
    owned_pid: Option<u32>,
    external: bool,
    base_url: Option<String>,
    last_activity: Instant,
    last_activity_unix: u64,
    last_failure: Option<String>,
    /// P4-C: pid of the detached idle supervisor this process recorded.
    supervisor_pid: Option<u32>,
}

pub struct MlxLifecycle {
    inner: Mutex<Inner>,
}

static GLOBAL: OnceLock<MlxLifecycle> = OnceLock::new();
static WATCHER_STARTED: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The process-wide lifecycle manager.
pub fn global() -> &'static MlxLifecycle {
    GLOBAL.get_or_init(|| MlxLifecycle {
        inner: Mutex::new(Inner {
            cfg: LifecycleConfig::from_env(),
            state: LifecycleState::Idle,
            child: None,
            owned_pid: None,
            external: false,
            base_url: None,
            last_activity: Instant::now(),
            last_activity_unix: now_unix(),
            last_failure: None,
            supervisor_pid: None,
        }),
    })
}

/// RAII guard counting in-flight inferences (INFERENCE state). The watcher
/// never shuts a server down while the counter is non-zero.
pub struct InferenceGuard;

impl Drop for InferenceGuard {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn begin_inference() -> InferenceGuard {
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    InferenceGuard
}

pub fn in_flight_count() -> usize {
    IN_FLIGHT.load(Ordering::SeqCst)
}

fn local_base_url(base_url: &str) -> bool {
    let host = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host = host.split('/').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    matches!(
        host,
        "127.0.0.1" | "localhost" | "0.0.0.0" | "::1" | "[::1]"
    )
}

fn extract_port(base_url: &str) -> u16 {
    // Strip the scheme FIRST: "http://host:port/..." must yield the port,
    // not fall back to the default.
    let stripped = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let hostport = stripped.split('/').next().unwrap_or("");
    hostport
        .split(':')
        .nth(1)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(8080)
}

/// Blocking health probe: GET {base}/models over a plain TCP socket.
///
/// Deliberately dependency-free (no reqwest, no curl): safe to call from
/// ANY thread context, including async main (reqwest::blocking panics when
/// dropped inside a tokio context) and immune to proxy env vars.
pub fn probe_endpoint(base_url: &str) -> bool {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};

    let stripped = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let (hostport, path) = match stripped.find('/') {
        Some(i) => (&stripped[..i], &stripped[i..]),
        None => (stripped, ""),
    };
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(80)),
        None => (hostport.to_string(), 80),
    };
    let addrs = match (host.as_str(), port).to_socket_addrs() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let addr = match addrs.into_iter().next() {
        Some(a) => a,
        None => return false,
    };
    let mut stream = match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let req_path = format!("{}/models", path.trim_end_matches('/'));
    let request =
        format!("GET {req_path} HTTP/1.0\r\nHost: {hostport}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut buf = vec![0u8; 512];
    match stream.read(&mut buf) {
        Ok(n) if n > 0 => {
            let head = String::from_utf8_lossy(&buf[..n]);
            // "HTTP/1.0 200" or "HTTP/1.1 200"
            head.split_whitespace()
                .nth(1)
                .map(|code| code.starts_with('2'))
                .unwrap_or(false)
        }
        _ => false,
    }
}

/// P4-C/D: true when `pid` is a zombie. A zombie is already dead (not
/// executing, holding no RAM) but still has a pid table entry because its
/// parent has not reaped it yet. `kill(pid, 0)` reports a zombie as alive,
/// which would make unload verification loop forever on a process that is
/// already gone; treating zombies as dead is the truthful semantics.
fn process_is_zombie(pid: u32) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "state="])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().starts_with('Z'),
        // ps could not report the pid: not a zombie we can prove.
        _ => false,
    }
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let signaled = unsafe { libc::kill(pid as i32, 0) == 0 };
        signaled && !process_is_zombie(pid)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

fn send_signal(pid: u32, kill_hard: bool) {
    #[cfg(unix)]
    {
        let sig = if kill_hard {
            libc::SIGKILL
        } else {
            libc::SIGTERM
        };
        unsafe { libc::kill(pid as i32, sig) };
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, kill_hard);
    }
}

fn read_state_file(path: &PathBuf) -> Option<PersistedState> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_state_file(path: &PathBuf, state: &PersistedState) {
    let json = match serde_json::to_string_pretty(state) {
        Ok(j) => j,
        Err(_) => return,
    };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

impl MlxLifecycle {
    /// Build an isolated manager with explicit configuration (used by tests
    /// and operator tooling; production code uses `global()`).
    pub fn with_config(cfg: LifecycleConfig) -> Self {
        MlxLifecycle {
            inner: Mutex::new(Inner {
                cfg,
                state: LifecycleState::Idle,
                child: None,
                owned_pid: None,
                external: false,
                base_url: None,
                last_activity: Instant::now(),
                last_activity_unix: now_unix(),
                last_failure: None,
                supervisor_pid: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("mlx lifecycle mutex poisoned")
    }

    /// Ensure a model server is ready for `base_url`. Starts a managed
    /// server when needed, reuses running servers, resets the idle timer.
    /// Fails truthfully when the server cannot be started.
    pub fn ensure_ready(&self, base_url: &str) -> anyhow::Result<()> {
        let cfg = self.lock().cfg.clone();
        if !cfg.enabled {
            return Ok(());
        }
        if std::env::var("DAK_LM_BACKEND").ok().as_deref() == Some("mock") {
            return Ok(());
        }
        if !local_base_url(base_url) {
            return Ok(()); // remote endpoints are never managed
        }

        let mut inner = self.lock();
        inner.base_url = Some(base_url.to_string());

        if probe_endpoint(base_url) {
            if inner.owned_pid.is_none() && !inner.external {
                // A server is already serving. If our own persisted state
                // says the kernel owns it (a prior process started it and
                // exited), re-adopt ownership so idle unload still applies.
                // Otherwise it is external and must never be killed.
                let adopted = read_state_file(&cfg.state_file).and_then(|p| {
                    if p.owned && p.base_url == base_url && p.pid.is_some() {
                        p.pid
                    } else {
                        None
                    }
                });
                match adopted {
                    Some(pid) if process_alive(pid) => {
                        inner.owned_pid = Some(pid);
                    }
                    _ => {
                        inner.external = true;
                    }
                }
            }
            inner.state = LifecycleState::ModelLoaded;
            inner.last_activity = Instant::now();
            inner.last_activity_unix = now_unix();
            self.persist_locked(&inner);
            drop(inner);
            self.start_watcher();
            return Ok(());
        }

        if inner.external {
            anyhow::bail!(
                "external model server at {base_url} is down; lifecycle does not manage external servers"
            );
        }

        // Cross-process stale ownership: previous kernel process died.
        if let Some(persisted) = read_state_file(&cfg.state_file) {
            if persisted.owned {
                match persisted.pid {
                    Some(pid) if process_alive(pid) => {
                        // Alive owned process but endpoint down: give it a
                        // moment (it may be starting), then treat as stuck.
                        let deadline = Instant::now() + Duration::from_secs(5);
                        while Instant::now() < deadline {
                            if probe_endpoint(base_url) {
                                inner.owned_pid = Some(pid);
                                inner.state = LifecycleState::ModelLoaded;
                                inner.last_activity = Instant::now();
                                inner.last_activity_unix = now_unix();
                                self.persist_locked(&inner);
                                drop(inner);
                                self.start_watcher();
                                return Ok(());
                            }
                            std::thread::sleep(READINESS_POLL);
                        }
                        // Orphan/stuck owned process: stop it before restart.
                        drop(inner);
                        let report = self.stop_owned_pid(pid, &cfg, base_url);
                        inner = self.lock();
                        if !report.process_gone {
                            inner.last_failure = Some(report.detail.clone());
                            anyhow::bail!(
                                "cannot start model server: previous owned process pid={} is stuck: {}",
                                pid,
                                report.detail
                            );
                        }
                        inner.last_failure = None;
                    }
                    _ => {
                        // Dead pid: clean the stale state file.
                        let _ = std::fs::remove_file(&cfg.state_file);
                    }
                }
            }
        }

        // MODEL_START
        inner.state = LifecycleState::ModelStart;
        let model = std::env::var("OPENAI_MODEL")
            .map_err(|_| anyhow::anyhow!("cannot start model server: OPENAI_MODEL is not set"))?;

        // Cross-process startup serialization (mission §8: two concurrent
        // pipeline-runs must not spawn two servers). If another kernel
        // process is already starting a server for this endpoint, wait for
        // ITS readiness instead of spawning a duplicate that would fail on
        // the occupied port.
        if let Some(p) = read_state_file(&cfg.state_file) {
            if p.state == LifecycleState::ModelStart
                && p.base_url == base_url
                && now_unix().saturating_sub(p.started_unix) < cfg.startup_timeout.as_secs()
            {
                drop(inner);
                let deadline = Instant::now() + cfg.startup_timeout;
                loop {
                    if probe_endpoint(base_url) {
                        let mut inner = self.lock();
                        inner.state = LifecycleState::ModelLoaded;
                        inner.last_activity = Instant::now();
                        inner.last_activity_unix = now_unix();
                        self.persist_locked(&inner);
                        return Ok(());
                    }
                    if Instant::now() > deadline {
                        anyhow::bail!(
                            "model server startup by another process did not complete within {:?}",
                            cfg.startup_timeout
                        );
                    }
                    std::thread::sleep(READINESS_POLL);
                }
            }
        }

        // Publish the ModelStart marker BEFORE spawning so concurrent
        // processes can observe it. Preserve any already-recorded
        // supervisor pid so this write never drops supervisor tracking.
        let prior_supervisor = read_state_file(&cfg.state_file).and_then(|p| p.supervisor_pid);
        write_state_file(
            &cfg.state_file,
            &PersistedState {
                pid: None,
                owned: true,
                state: LifecycleState::ModelStart,
                model: model.clone(),
                base_url: base_url.to_string(),
                started_unix: now_unix(),
                last_activity_unix: now_unix(),
                supervisor_pid: prior_supervisor,
            },
        );

        let port = extract_port(base_url);
        let log_path = std::env::temp_dir().join("dak_mlx_server.log");
        let open_log = || {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .ok()
        };
        let mut cmd = Command::new(&cfg.server_command);
        cmd.args([
            "--model",
            &model,
            "--port",
            &port.to_string(),
            "--decode-concurrency",
            "1",
            "--prompt-concurrency",
            "1",
        ])
        .stdout(open_log().map(Stdio::from).unwrap_or(Stdio::null()))
        .stderr(open_log().map(Stdio::from).unwrap_or(Stdio::null()));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // P4-C: the server owns the model and its lifecycle is managed
            // by the supervisor — it must not die with the CLI's process
            // group (terminal hangs, script/teardown group-kills). Detach it
            // into its own session; the supervisor unloads it after idle.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        let child = cmd.spawn();

        let child = match child {
            Ok(c) => c,
            Err(e) => {
                inner.state = LifecycleState::Idle;
                inner.last_failure = Some(format!("spawn failed: {e}"));
                anyhow::bail!("cannot start model server '{}': {e}", cfg.server_command);
            }
        };
        let pid = child.id();
        inner.child = Some(child);
        inner.owned_pid = Some(pid);
        self.persist_locked(&inner);
        drop(inner);
        self.start_watcher();

        // Wait for MODEL_LOADED (readiness), bounded.
        let deadline = Instant::now() + cfg.startup_timeout;
        loop {
            if probe_endpoint(base_url) {
                let mut inner = self.lock();
                inner.state = LifecycleState::ModelLoaded;
                inner.last_activity = Instant::now();
                inner.last_activity_unix = now_unix();
                inner.last_failure = None;
                self.persist_locked(&inner);
                drop(inner);
                // P4-C: ephemeral CLI processes exit after their inference;
                // the detached supervisor keeps idle unload deterministic
                // without requiring `lm-lifecycle watch`.
                self.ensure_supervisor();
                return Ok(());
            }
            let mut inner = self.lock();
            let alive = inner
                .child
                .as_mut()
                .map(|c| c.try_wait().ok().flatten().is_none())
                .unwrap_or(false);
            if !alive {
                inner.state = LifecycleState::Idle;
                inner.owned_pid = None;
                inner.child = None;
                inner.last_failure = Some("server process exited during startup".to_string());
                let _ = std::fs::remove_file(&cfg.state_file);
                anyhow::bail!(
                    "model server pid {pid} exited during startup (see {})",
                    log_path.display()
                );
            }
            drop(inner);
            if Instant::now() > deadline {
                // Startup failure: clean up and report truthfully.
                let report = self.stop_owned_pid(pid, &cfg, base_url);
                let mut inner = self.lock();
                inner.state = LifecycleState::Idle;
                inner.owned_pid = None;
                inner.child = None;
                inner.last_failure = Some(format!(
                    "startup timeout after {:?}: {}",
                    cfg.startup_timeout, report.detail
                ));
                anyhow::bail!(
                    "model server not ready after {:?} (pid {pid}); {}",
                    cfg.startup_timeout,
                    report.detail
                );
            }
            std::thread::sleep(READINESS_POLL);
        }
    }

    /// Reset the idle timer (called after every completed inference).
    pub fn record_activity(&self) {
        let mut inner = self.lock();
        inner.last_activity = Instant::now();
        inner.last_activity_unix = now_unix();
        self.persist_locked(&inner);
    }

    pub fn status(&self) -> LifecycleStatus {
        let inner = self.lock();
        let endpoint_up = inner
            .base_url
            .as_deref()
            .map(probe_endpoint)
            .unwrap_or(false);
        LifecycleStatus {
            state: inner.state,
            enabled: inner.cfg.enabled,
            pid: inner.owned_pid,
            owned: inner.owned_pid.is_some() && !inner.external,
            external: inner.external,
            endpoint_up,
            idle_secs: inner.last_activity.elapsed().as_secs(),
            timeout_secs: inner.cfg.idle_timeout.as_secs(),
            in_flight: in_flight_count(),
            last_failure: inner.last_failure.clone(),
        }
    }

    /// Decision function behind the idle watcher (exposed for deterministic
    /// tests). True only when ALL hold: management enabled, timeout > 0,
    /// server kernel-owned (never external), loaded, no inference in flight,
    /// idle >= timeout counted from the last completed inference.
    pub fn idle_shutdown_due(&self) -> bool {
        let inner = self.lock();
        inner.cfg.enabled
            && inner.cfg.idle_timeout > Duration::ZERO
            && inner.owned_pid.is_some()
            && !inner.external
            && inner.state == LifecycleState::ModelLoaded
            && in_flight_count() == 0
            && inner.last_activity.elapsed() >= inner.cfg.idle_timeout
    }

    /// Cross-process idle sync: short-lived CLI processes perform the actual
    /// inferences and persist last_activity into the state file; a resident
    /// watcher adopts the freshest timestamp so the idle timeout counts from
    /// the globally last completed request, not from this process's own.
    pub fn sync_activity_from_state_file(&self) {
        let mut inner = self.lock();
        if let Some(persisted) = read_state_file(&inner.cfg.state_file) {
            if persisted.last_activity_unix > inner.last_activity_unix {
                let age = now_unix().saturating_sub(persisted.last_activity_unix);
                inner.last_activity = Instant::now() - Duration::from_secs(age);
                inner.last_activity_unix = persisted.last_activity_unix;
            }
        }
    }

    /// Graceful shutdown of a kernel-owned server. Never reports success
    /// without verifying both process and endpoint disappearance.
    ///
    /// Works cross-process: if THIS process does not hold the child handle
    /// but the persisted state says the kernel owns a live server (e.g. the
    /// owning CLI process already exited), ownership is adopted for the
    /// purpose of stopping it by pid.
    pub fn stop_managed_server(&self) -> anyhow::Result<ShutdownReport> {
        let (pid_opt, cfg, base_url) = {
            let mut inner = self.lock();
            if inner.owned_pid.is_none() && !inner.external {
                // Adoption path for orphaned owned servers.
                if let Some(p) = read_state_file(&inner.cfg.state_file) {
                    if p.owned && p.state != LifecycleState::Unloaded {
                        if let Some(pid) = p.pid {
                            // P4-D: before adopting (and later signaling) a
                            // pid this process did not spawn, verify it
                            // still identifies as the managed server. This
                            // closes the pid-reuse / stale-state hazard: an
                            // unrelated process can never be SIGTERMed via
                            // lifecycle adoption.
                            if process_alive(pid) && process_looks_like_server(pid, &inner.cfg) {
                                inner.owned_pid = Some(pid);
                                if inner.base_url.is_none() {
                                    inner.base_url = Some(p.base_url.clone());
                                }
                            }
                        }
                    }
                }
            }
            if inner.external || inner.owned_pid.is_none() {
                return Ok(ShutdownReport {
                    attempted: false,
                    pid: None,
                    escalated_to_kill: false,
                    process_gone: true,
                    endpoint_down: true,
                    detail: "no kernel-owned server to stop".to_string(),
                });
            }
            inner.state = LifecycleState::ShuttingDown;
            self.persist_locked(&inner);
            (inner.owned_pid, inner.cfg.clone(), inner.base_url.clone())
        };
        let pid = pid_opt.expect("checked above");
        let report = self.stop_owned_pid(pid, &cfg, base_url.as_deref().unwrap_or(""));
        let mut inner = self.lock();
        if report.verified() {
            inner.state = LifecycleState::Unloaded;
            inner.owned_pid = None;
            inner.child = None;
            inner.last_failure = None;
            self.persist_locked(&inner);
            Ok(report)
        } else {
            inner.last_failure = Some(report.detail.clone());
            self.persist_locked(&inner);
            anyhow::bail!("unload not verified: {}", report.detail)
        }
    }

    /// Core stop sequence shared by explicit stop, orphan recovery and
    /// startup-timeout cleanup: SIGTERM -> bounded wait -> SIGKILL fallback
    /// -> verify process + endpoint disappearance.
    ///
    /// Zombie-safe: an owned child that already exited is reaped through
    /// try_wait/wait before liveness checks; kill(pid, 0) alone would report
    /// a zombie as alive and fake a "survived SIGKILL" failure.
    fn stop_owned_pid(&self, pid: u32, cfg: &LifecycleConfig, base_url: &str) -> ShutdownReport {
        send_signal(pid, false);

        let grace_deadline = Instant::now() + cfg.shutdown_grace;
        let mut escalated = false;
        loop {
            // Reap first: our child may already be a zombie.
            let reaped = {
                let mut inner = self.lock();
                match inner.child.as_mut() {
                    Some(child) => matches!(child.try_wait(), Ok(Some(_))),
                    None => false,
                }
            };
            if reaped {
                break;
            }
            if !process_alive(pid) {
                break;
            }
            if Instant::now() > grace_deadline {
                if !escalated {
                    escalated = true;
                    send_signal(pid, true); // bounded fallback for stuck process
                    std::thread::sleep(Duration::from_millis(300));
                    continue;
                }
                let reaped_after_kill = {
                    let mut inner = self.lock();
                    match inner.child.as_mut() {
                        Some(child) => matches!(child.try_wait(), Ok(Some(_))),
                        None => false,
                    }
                };
                if reaped_after_kill || !process_alive(pid) {
                    break;
                }
                return ShutdownReport {
                    attempted: true,
                    pid: Some(pid),
                    escalated_to_kill: true,
                    process_gone: false,
                    endpoint_down: base_url.is_empty() || !probe_endpoint(base_url),
                    detail: format!("pid {pid} survived SIGTERM and SIGKILL; process NOT unloaded"),
                };
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        // Fully reap the child if we own its handle (prevents zombies).
        {
            let mut inner = self.lock();
            if let Some(mut child) = inner.child.take() {
                let _ = child.wait();
            }
            if inner.owned_pid == Some(pid) {
                inner.owned_pid = None;
            }
        }

        // Verify endpoint really down (a different process could hold it).
        let endpoint_down = if base_url.is_empty() {
            true
        } else {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if !probe_endpoint(base_url) {
                    break true;
                }
                if Instant::now() > deadline {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        };

        ShutdownReport {
            attempted: true,
            pid: Some(pid),
            escalated_to_kill: escalated,
            process_gone: !process_alive(pid),
            endpoint_down,
            detail: if escalated {
                "SIGTERM ignored; escalated to SIGKILL".to_string()
            } else {
                "graceful SIGTERM shutdown".to_string()
            },
        }
    }

    fn persist_locked(&self, inner: &Inner) {
        // Preserve the supervisor tracking across persists. A supervisor
        // recorded by THIS process (inner) always wins over whatever the
        // file still says — the file may carry a stale dead supervisor pid
        // from a previous process.
        let supervisor_pid = inner
            .supervisor_pid
            .or_else(|| read_state_file(&inner.cfg.state_file).and_then(|p| p.supervisor_pid));
        let persisted = PersistedState {
            pid: inner.owned_pid,
            owned: inner.owned_pid.is_some(),
            state: inner.state,
            model: std::env::var("OPENAI_MODEL").unwrap_or_default(),
            base_url: inner.base_url.clone().unwrap_or_default(),
            started_unix: now_unix(),
            last_activity_unix: inner.last_activity_unix,
            supervisor_pid,
        };
        if persisted.owned {
            write_state_file(&inner.cfg.state_file, &persisted);
        } else if inner.state == LifecycleState::Unloaded || inner.owned_pid.is_none() {
            // Keep the file only while ownership matters; Unloaded is
            // recorded so operators can see the last transition.
            write_state_file(&inner.cfg.state_file, &persisted);
        }
    }

    /// P4-C: adopt an owned server into this process's lifecycle state for
    /// the sole purpose of supervising/stopping it. Does NOT bump activity
    /// (a supervisor must never reset the idle timer by looking at the
    /// server).
    pub fn adopt_for_supervision(&self, pid: u32, base_url: Option<String>) {
        let mut inner = self.lock();
        inner.owned_pid = Some(pid);
        inner.external = false;
        inner.state = LifecycleState::ModelLoaded;
        if inner.base_url.is_none() {
            inner.base_url = base_url;
        }
    }

    /// P4-C: ensure a detached idle supervisor exists for a kernel-owned
    /// server. Deduplicated through the persisted supervisor_pid: a live
    /// recorded supervisor is reused; a dead one is replaced. The supervisor
    /// is a detached child of THIS process (stdio nulled) and survives the
    /// CLI exiting, so idle unload stays deterministic for ephemeral CLIs.
    pub fn ensure_supervisor(&self) {
        let need_spawn = {
            let inner = self.lock();
            let state_file = inner.cfg.state_file.clone();
            !matches!(
                read_state_file(&state_file).and_then(|p| p.supervisor_pid),
                Some(spid) if process_alive(spid)
            )
        };
        if !need_spawn {
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(_) => return,
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("__lifecycle-supervise")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Detach into its own session/process group: the supervisor must
            // survive group-wide cleanup of the CLI that spawned it
            // (kill -- -PGID and similar process-group teardown). Safe: the
            // closure runs in the forked child before exec.
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        let child = cmd.spawn();
        if let Ok(child) = child {
            let spid = child.id();
            let mut inner = self.lock();
            inner.supervisor_pid = Some(spid);
            self.persist_locked(&inner);
        }
    }

    /// Daemon idle watcher. Started once per process. Never stops a server
    /// with in-flight inference; re-checks freshness inside the lock.
    fn start_watcher(&self) {
        if WATCHER_STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::Builder::new()
            .name("mlx-idle-watcher".to_string())
            .spawn(|| loop {
                std::thread::sleep(WATCHER_TICK);
                global().sync_activity_from_state_file();
                if !global().idle_shutdown_due() {
                    continue;
                }
                let timeout = global().lock().cfg.idle_timeout;
                eprintln!(
                    "[mlx-lifecycle] idle timeout ({:?}) reached; graceful shutdown",
                    timeout
                );
                match global().stop_managed_server() {
                    Ok(report) => {
                        eprintln!(
                            "[mlx-lifecycle] MODEL_UNLOADED pid={:?} escalated={} verified={}",
                            report.pid,
                            report.escalated_to_kill,
                            report.verified()
                        );
                    }
                    Err(e) => {
                        eprintln!("[mlx-lifecycle] unload FAILED: {e}");
                    }
                }
            })
            .expect("idle watcher thread");
    }
}

// ── Static convenience API used by the inference choke points ───────────────

pub fn ensure_ready(base_url: &str) -> anyhow::Result<()> {
    global().ensure_ready(base_url)
}

pub fn record_activity() {
    global().record_activity()
}

pub fn stop_managed_server() -> anyhow::Result<ShutdownReport> {
    global().stop_managed_server()
}

pub fn status() -> LifecycleStatus {
    global().status()
}

/// Cross-process observability: the persisted lifecycle snapshot written by
/// whichever process currently owns (or last owned) the managed server.
pub fn persisted_snapshot() -> Option<PersistedState> {
    let cfg = LifecycleConfig::from_env();
    read_state_file(&cfg.state_file)
}

/// P4-D: verify that a pid we are about to treat as the owned server
/// really looks like the managed server process before signaling it.
/// Guards against pid reuse / stale state: we never SIGTERM a process whose
/// command line does not identify as the model server. Uses `ps` (read-only,
/// no shell).
///
/// Strictness rule (P4): the EXECUTABLE (first command-line token) must be
/// a python interpreter AND the arguments must reference the mlx server
/// (configured server-command basename or "mlx_lm"). Matching free text
/// anywhere in the command line is unsafe: a shell whose command text merely
/// mentions an mlx path would otherwise qualify.
pub fn process_looks_like_server(pid: u32, cfg: &LifecycleConfig) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output();
    let cmdline = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_lowercase(),
        _ => return false,
    };
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return false;
    }
    let exe_token = cmdline.split_whitespace().next().unwrap_or("");
    let exe_name = exe_token.split('/').next_back().unwrap_or("");
    if !exe_name.contains("python") {
        return false;
    }
    let want = cfg
        .server_command
        .split('/')
        .next_back()
        .unwrap_or("mlx_lm.server")
        .to_lowercase();
    cmdline.contains(&want) || cmdline.contains("mlx_lm") || cmdline.contains("mlx")
}

/// P4-C: persist a truthful Unloaded marker when the owned server is already
/// gone (keeps the state file from freezing at ShuttingDown).
fn persist_unloaded_marker(cfg: &LifecycleConfig, prior: &PersistedState) {
    write_state_file(
        &cfg.state_file,
        &PersistedState {
            pid: None,
            owned: false,
            state: LifecycleState::Unloaded,
            model: prior.model.clone(),
            base_url: prior.base_url.clone(),
            started_unix: prior.started_unix,
            last_activity_unix: prior.last_activity_unix,
            supervisor_pid: prior.supervisor_pid,
        },
    );
}

/// P4-C: the detached idle supervisor body. Runs as its own process
/// (`deterministic_ai_kernel __lifecycle-supervise`), spawned by
/// `ensure_supervisor`. It owns NO server handle; it reads the persisted
/// state, and after the configured idle elapses (with no recorded activity)
/// it gracefully unloads the server and exits. Exit codes:
///   0 = nothing to supervise, already unloaded, or unload verified
///   1 = unload attempted but NOT verified (honest failure)
///
/// Safety properties enforced here:
///   - never SIGKILL first: stop_managed_server sends SIGTERM and only
///     escalates after the bounded grace period;
///   - never kills an active inference: activity is re-synced from the state
///     file every tick and the idle check requires last_activity older than
///     the timeout;
///   - no double-unload race: exits immediately once the persisted state is
///     Unloaded or the server pid is already gone;
///   - refuses to signal a pid that no longer identifies as the server.
pub fn run_supervisor_loop() -> i32 {
    let cfg = LifecycleConfig::from_env();
    let debug = std::env::var("DAK_MLX_SUPERVISOR_DEBUG").is_ok();
    macro_rules! dlog {
        ($($arg:tt)*) => {
            if debug { eprintln!("[supervisor] {}", format_args!($($arg)*)); }
        };
    }
    if !cfg.enabled || cfg.idle_timeout == Duration::ZERO {
        dlog!("exit: disabled or timeout=0");
        return 0;
    }
    // Deduplicate: if another supervisor is recorded and alive, and it is
    // not us, step down (only one supervisor unloads).
    if let Some(p) = read_state_file(&cfg.state_file) {
        if let Some(spid) = p.supervisor_pid {
            if spid != std::process::id() as u32 && process_alive(spid) {
                dlog!("exit: another supervisor {spid} owns this");
                return 0;
            }
        }
    }

    let mut unload_failures = 0u32;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let snap = match read_state_file(&cfg.state_file) {
            Some(p) => p,
            None => {
                dlog!("exit: no state file");
                return 0;
            }
        };
        if snap.state == LifecycleState::Unloaded {
            dlog!("exit: already Unloaded");
            return 0;
        }
        if !snap.owned {
            dlog!("exit: not owned (external)");
            return 0;
        }
        let server_pid = match snap.pid {
            Some(p) if process_alive(p) => p,
            other => {
                if snap.owned {
                    // The owned server is gone (we unloaded it, another
                    // actor stopped it, or it crashed). Record the truthful
                    // terminal state instead of leaving a stale ModelLoaded.
                    persist_unloaded_marker(&cfg, &snap);
                }
                dlog!("exit: server pid gone/absent ({other:?})");
                return 0;
            }
        };
        if !process_looks_like_server(server_pid, &cfg) {
            dlog!("exit: pid {server_pid} does not identify as server");
            return 0;
        }
        global().adopt_for_supervision(server_pid, Some(snap.base_url.clone()));
        global().sync_activity_from_state_file();
        if !global().idle_shutdown_due() {
            dlog!("idle not due yet (pid {server_pid})");
            continue;
        }
        dlog!("idle due -> stopping pid {server_pid}");
        match global().stop_managed_server() {
            Ok(report) if report.verified() => {
                dlog!("unload verified");
                return 0;
            }
            Ok(report) => {
                dlog!("unload NOT verified: {}", report.detail);
                return 1;
            }
            Err(e) => {
                dlog!("stop error: {e}");
                unload_failures += 1;
                if unload_failures >= 5 {
                    return 1; // honest: unload could not be verified
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Env vars are process-global: every test that mutates them must hold
    /// this lock so parallel lib tests cannot race on the environment.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn local_base_url_detection() {
        assert!(local_base_url("http://127.0.0.1:8081/v1"));
        assert!(local_base_url("http://localhost:8080/v1"));
        assert!(!local_base_url("http://10.0.0.5:8081/v1"));
        assert!(!local_base_url("https://api.openai.com/v1"));
    }

    #[test]
    fn port_extraction() {
        assert_eq!(extract_port("http://127.0.0.1:8081/v1"), 8081);
        assert_eq!(extract_port("http://127.0.0.1:9999/v1"), 9999);
        assert_eq!(extract_port("no-port-here"), 8080);
    }

    #[test]
    fn config_env_contract_sequential() {
        // Pure-parse contract: deterministic fake lookups, NO mutation of the
        // process environment (mutating MLX_LIFECYCLE here would flip
        // lifecycle on/off for every concurrently running test).

        let empty = |_: &str| None;

        // Defaults.
        let cfg = LifecycleConfig::parse(&empty);
        assert_eq!(cfg.idle_timeout, Duration::from_secs(120));
        assert_eq!(cfg.server_command, "mlx_lm.server");
        assert!(cfg.enabled);

        // Configured timeout is actually used.
        let cfg =
            LifecycleConfig::parse(&|k| (k == "MLX_IDLE_TIMEOUT_SECS").then(|| "77".to_string()));
        assert_eq!(cfg.idle_timeout, Duration::from_secs(77));

        // 0 disables automatic unload.
        let cfg =
            LifecycleConfig::parse(&|k| (k == "MLX_IDLE_TIMEOUT_SECS").then(|| "0".to_string()));
        assert_eq!(cfg.idle_timeout, Duration::ZERO);

        // Off switch variants.
        for off_val in ["off", "0", "disabled", "OFF"] {
            let cfg =
                LifecycleConfig::parse(&|k| (k == "MLX_LIFECYCLE").then(|| off_val.to_string()));
            assert!(!cfg.enabled, "'{off_val}' must disable lifecycle");
        }

        // Explicit on.
        let cfg = LifecycleConfig::parse(&|k| (k == "MLX_LIFECYCLE").then(|| "on".to_string()));
        assert!(cfg.enabled);

        // Custom server command + state file.
        let cfg = LifecycleConfig::parse(&|k| match k {
            "MLX_SERVER_COMMAND" => Some("/custom/server".to_string()),
            "DAK_MLX_LIFECYCLE_STATE" => Some("/tmp/custom_state.json".to_string()),
            _ => None,
        });
        assert_eq!(cfg.server_command, "/custom/server");
        assert_eq!(cfg.state_file, PathBuf::from("/tmp/custom_state.json"));

        // Empty server command falls back to the default.
        let cfg =
            LifecycleConfig::parse(&|k| (k == "MLX_SERVER_COMMAND").then(|| "  ".to_string()));
        assert_eq!(cfg.server_command, "mlx_lm.server");
    }

    #[test]
    fn probe_endpoint_fails_closed_for_dead_port() {
        assert!(!probe_endpoint("http://127.0.0.1:9"));
    }

    #[test]
    fn inference_guard_counts_in_flight() {
        let before = in_flight_count();
        let g1 = begin_inference();
        let g2 = begin_inference();
        assert_eq!(in_flight_count(), before + 2);
        drop(g1);
        drop(g2);
        assert_eq!(in_flight_count(), before);
    }

    #[test]
    fn ensure_ready_is_noop_for_remote_urls() {
        assert!(ensure_ready("https://api.example.com/v1").is_ok());
    }

    #[test]
    fn ensure_ready_is_noop_when_disabled() {
        // Explicit disabled config — no process-env mutation (the cargo
        // baseline keeps MLX_LIFECYCLE=off for every test in this binary).
        let lc = MlxLifecycle {
            inner: Mutex::new(Inner {
                cfg: LifecycleConfig {
                    idle_timeout: Duration::from_secs(120),
                    startup_timeout: Duration::from_secs(300),
                    shutdown_grace: Duration::from_secs(20),
                    enabled: false,
                    server_command: "mlx_lm.server".to_string(),
                    state_file: std::env::temp_dir().join("dak_mlx_noop_state.json"),
                },
                state: LifecycleState::Idle,
                child: None,
                owned_pid: None,
                external: false,
                base_url: None,
                last_activity: Instant::now(),
                last_activity_unix: now_unix(),
                last_failure: None,
                supervisor_pid: None,
            }),
        };
        assert!(lc.ensure_ready("http://127.0.0.1:9").is_ok());
        assert_eq!(lc.status().state, LifecycleState::Idle);
        assert!(lc.status().pid.is_none());
    }

    #[test]
    fn startup_failure_is_truthful() {
        let _g = ENV_LOCK.lock().unwrap();
        // A command that cannot exist must produce an honest error and
        // never leave the manager in ModelLoaded state.
        let tmp_state =
            std::env::temp_dir().join(format!("dak_mlx_test_state_{}.json", std::process::id()));
        let lc = MlxLifecycle {
            inner: Mutex::new(Inner {
                cfg: LifecycleConfig {
                    idle_timeout: Duration::from_secs(1),
                    startup_timeout: Duration::from_secs(2),
                    shutdown_grace: Duration::from_secs(2),
                    enabled: true,
                    server_command: "/nonexistent/mlx_server_binary".to_string(),
                    state_file: tmp_state.clone(),
                },
                state: LifecycleState::Idle,
                child: None,
                owned_pid: None,
                external: false,
                base_url: None,
                last_activity: Instant::now(),
                last_activity_unix: now_unix(),
                last_failure: None,
                supervisor_pid: None,
            }),
        };
        // Save/restore OPENAI_MODEL: never leave a fake model path visible
        // to other tests after this one finishes.
        let saved_model = std::env::var("OPENAI_MODEL").ok();
        std::env::set_var("OPENAI_MODEL", "/tmp/nonexistent-model");
        let err = lc
            .ensure_ready("http://127.0.0.1:9")
            .expect_err("spawn must fail");
        assert!(err.to_string().contains("cannot start model server"));
        assert_eq!(lc.status().state, LifecycleState::Idle);
        assert!(lc.status().last_failure.is_some());
        match saved_model {
            Some(v) => std::env::set_var("OPENAI_MODEL", v),
            None => std::env::remove_var("OPENAI_MODEL"),
        }
        let _ = std::fs::remove_file(&tmp_state);
    }

    /// A config whose state_file points at a path that never exists, so
    /// these unit tests can NEVER adopt or touch a real/live lifecycle
    /// server that a concurrent acceptance run may own (the default
    /// state-file path is shared process-globally).
    fn isolated_test_cfg() -> LifecycleConfig {
        let mut cfg = LifecycleConfig::from_env();
        cfg.state_file = std::env::temp_dir().join(format!(
            "dak_mlx_iso_state_{}_{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        cfg
    }

    #[test]
    fn stop_without_owned_server_is_safe_noop() {
        let lc = MlxLifecycle {
            inner: Mutex::new(Inner {
                cfg: isolated_test_cfg(),
                state: LifecycleState::Idle,
                child: None,
                owned_pid: None,
                external: false,
                base_url: None,
                last_activity: Instant::now(),
                last_activity_unix: now_unix(),
                last_failure: None,
                supervisor_pid: None,
            }),
        };
        let report = lc.stop_managed_server().expect("noop stop");
        assert!(!report.attempted);
    }

    #[test]
    fn external_server_is_never_stopped() {
        let lc = MlxLifecycle {
            inner: Mutex::new(Inner {
                cfg: LifecycleConfig::from_env(),
                state: LifecycleState::ModelLoaded,
                child: None,
                owned_pid: None,
                external: true,
                base_url: Some("http://127.0.0.1:1".to_string()),
                last_activity: Instant::now(),
                last_activity_unix: now_unix(),
                last_failure: None,
                supervisor_pid: None,
            }),
        };
        let report = lc.stop_managed_server().expect("external stop is noop");
        assert!(!report.attempted, "external server must never be touched");
    }
}
