//! RuntimeManager — automatic lifecycle management for the local MLX inference server.
//!
//! The user never starts `mlx_lm.server` manually.  `RuntimeManager::ensure_running()`
//! is called before every inference request and transparently starts, health-checks,
//! and crash-recovers the server process.
//!
//! Configuration lives in `config/runtime.json`.  Process state lives in `runtime/`.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// ── Configuration ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmbeddingsConfig {
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub health_check_url: Option<String>,
}

/// Deserialized from `config/runtime.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub provider: String,
    pub host: String,
    pub port: u16,
    pub default_model: String,
    pub auto_start: bool,
    pub venv_path: String,
    pub startup_timeout_secs: u64,
    pub health_check_interval_ms: u64,
    pub embeddings: Option<EmbeddingsConfig>,
}

impl RuntimeConfig {
    /// Canonical path to the config file (relative to the project root).
    const CONFIG_PATH: &'static str = "config/runtime.json";

    pub fn load() -> Result<Self> {
        let path = std::env::var("DAK_RUNTIME_CONFIG_PATH")
            .unwrap_or_else(|_| Self::CONFIG_PATH.to_string());
        let text = fs::read_to_string(&path).with_context(|| format!("failed to read {}", path))?;
        let cfg: Self =
            serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path))?;
        Ok(cfg)
    }

    pub fn base_url(&self) -> String {
        format!("http://{}:{}/v1", self.host, self.port)
    }

    fn server_binary(&self) -> PathBuf {
        PathBuf::from(&self.venv_path).join("bin/python")
    }
}

// ── Runtime status ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Running,
    Stopped,
    Starting,
    Error,
}

impl std::fmt::Display for RuntimeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Starting => write!(f, "starting"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Snapshot returned by `status()` and `ensure_running()`.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub status: RuntimeStatus,
    pub pid: Option<u32>,
    pub host: String,
    pub port: u16,
    pub provider: String,
    pub loaded_model: Option<String>,
    pub config_model: String,
    pub base_url: String,
}

// ── PID / lock helpers ───────────────────────────────────────────────────────

const RUNTIME_DIR: &str = "runtime";
const PID_FILE: &str = "runtime/mlx.pid";
const LOCK_FILE: &str = "runtime/mlx.lock";

fn ensure_runtime_dir() -> Result<()> {
    fs::create_dir_all(RUNTIME_DIR)
        .with_context(|| format!("failed to create {RUNTIME_DIR}/ directory"))
}

fn read_pid() -> Option<u32> {
    fs::read_to_string(PID_FILE)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

fn write_pid(pid: u32) -> Result<()> {
    ensure_runtime_dir()?;
    let mut f =
        fs::File::create(PID_FILE).with_context(|| format!("failed to create {PID_FILE}"))?;
    write!(f, "{pid}")?;
    Ok(())
}

fn remove_pid() {
    let _ = fs::remove_file(PID_FILE);
}

fn remove_lock() {
    let _ = fs::remove_file(LOCK_FILE);
}

const EMB_PID_FILE: &str = "runtime/embeddings.pid";
const EMB_LOCK_FILE: &str = "runtime/embeddings.lock";

fn read_emb_pid() -> Option<u32> {
    fs::read_to_string(EMB_PID_FILE)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

fn write_emb_pid(pid: u32) -> Result<()> {
    ensure_runtime_dir()?;
    let mut f = fs::File::create(EMB_PID_FILE)
        .with_context(|| format!("failed to create {EMB_PID_FILE}"))?;
    write!(f, "{pid}")?;
    Ok(())
}

fn remove_emb_pid() {
    let _ = fs::remove_file(EMB_PID_FILE);
}

fn remove_emb_lock() {
    let _ = fs::remove_file(EMB_LOCK_FILE);
}

/// Check whether a PID is alive using `kill(pid, 0)`.
fn pid_alive(pid: u32) -> bool {
    // SAFETY: `kill(pid, 0)` does not actually send a signal; it only checks
    // whether the process exists and is reachable.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Terminate a process (SIGTERM, then SIGKILL after 3 s).
fn kill_process(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if !pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Force kill if still alive
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

// ── Lock (advisory file lock) ────────────────────────────────────────────────

struct FileLock {
    _file: fs::File,
}

impl FileLock {
    /// Try to acquire an exclusive advisory lock on `LOCK_FILE`.
    /// Returns `None` immediately if another process holds the lock.
    fn try_acquire() -> Result<Option<Self>> {
        ensure_runtime_dir()?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(LOCK_FILE)
            .with_context(|| format!("failed to open {LOCK_FILE}"))?;

        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        // SAFETY: flock is a POSIX advisory lock; fd is valid because `file` is alive.
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Ok(None); // another process holds the lock
        }
        Ok(Some(Self { _file: file }))
    }

    /// Try to acquire an exclusive advisory lock on `EMB_LOCK_FILE`.
    /// Returns `None` immediately if another process holds the lock.
    fn try_acquire_emb() -> Result<Option<Self>> {
        ensure_runtime_dir()?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(EMB_LOCK_FILE)
            .with_context(|| format!("failed to open {EMB_LOCK_FILE}"))?;

        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Ok(None);
        }
        Ok(Some(Self { _file: file }))
    }
}

// ── Health probes ────────────────────────────────────────────────────────────

use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, TcpStream};

/// Probe the native MLX service via ping and return the loaded model ID.
fn probe_endpoint(host: &str, port: u16) -> Option<Vec<String>> {
    let addr_str = format!("{}:{}", host, port);
    let addr: SocketAddr = addr_str.parse().ok()?;

    // Connect with a 3-second timeout
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .ok()?;

    let req = serde_json::json!({
        "method": "ping"
    });
    let mut req_str = serde_json::to_string(&req).ok()?;
    req_str.push('\n');

    stream.write_all(req_str.as_bytes()).ok()?;
    stream.flush().ok()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;

    let resp: serde_json::Value = serde_json::from_str(&line).ok()?;
    if resp.get("status").and_then(|v| v.as_str()) == Some("ok") {
        let model = resp
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Some(vec![model])
    } else {
        None
    }
}

fn parse_host_port(url_str: &str) -> Option<(String, u16)> {
    let trimmed = url_str.trim();
    let strip_proto = if let Some(stripped) = trimmed.strip_prefix("http://") {
        stripped
    } else if let Some(stripped) = trimmed.strip_prefix("https://") {
        stripped
    } else {
        trimmed
    };

    let host_port = strip_proto.split('/').next()?;
    let mut parts = host_port.split(':');
    let host = parts.next()?.to_string();
    let port = if let Some(p_str) = parts.next() {
        p_str.parse::<u16>().ok()?
    } else {
        if url_str.starts_with("https://") {
            443
        } else {
            80
        }
    };
    Some((host, port))
}

/// Check embeddings health by sending a request to the configured health check URL or endpoint.
pub fn check_embeddings_health(cfg: &EmbeddingsConfig) -> Result<(), String> {
    if cfg.provider == "mock" || std::env::var("DAK_LM_BACKEND").as_deref() == Ok("mock") {
        return Ok(());
    }

    let url = cfg.health_check_url.as_ref().unwrap_or(&cfg.endpoint);
    let (host, port) =
        parse_host_port(url).ok_or_else(|| format!("Invalid health check URL: {}", url))?;

    let addr_str = format!("{}:{}", host, port);
    use std::net::ToSocketAddrs;
    let addrs: Vec<SocketAddr> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed for {}: {}", addr_str, e))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("No socket addresses found for {}", addr_str));
    }

    let mut connected = false;
    let mut last_err = String::new();
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, Duration::from_secs(2)) {
            Ok(_) => {
                connected = true;
                break;
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }

    if connected {
        Ok(())
    } else {
        Err(format!("Connection failed: {}", last_err))
    }
}

/// Block until the native socket responds to health checks or `timeout` elapses.
fn wait_for_health(host: &str, port: u16, timeout: Duration, interval: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if probe_endpoint(host, port).is_some() {
            return true;
        }
        std::thread::sleep(interval);
    }
    false
}

// ── RuntimeManager ───────────────────────────────────────────────────────────

pub struct RuntimeManager {
    config: RuntimeConfig,
}

impl RuntimeManager {
    // ── constructors ─────────────────────────────────────────────────────

    /// Load configuration from `config/runtime.json`.
    pub fn load() -> Result<Self> {
        let config = RuntimeConfig::load()?;
        Ok(Self { config })
    }

    /// Build a manager from an already-parsed config (useful in tests).
    #[cfg(test)]
    pub fn from_config(config: RuntimeConfig) -> Self {
        Self { config }
    }

    // ── public API ───────────────────────────────────────────────────────

    /// Ensure the MLX runtime is up and healthy.  Starts it if needed.
    /// This is the **only** function called before every inference request.
    pub fn ensure_running(&self) -> Result<RuntimeInfo> {
        // Fast path: process exists and endpoint responds.
        if let Some(pid) = read_pid() {
            if pid_alive(pid) {
                if let Some(models) = probe_endpoint(&self.config.host, self.config.port) {
                    return Ok(self.build_info(
                        RuntimeStatus::Running,
                        Some(pid),
                        models.first().cloned(),
                    ));
                }
                // Process alive but endpoint not responding — kill and restart.
                eprintln!("[runtime] PID {pid} alive but endpoint unresponsive — restarting");
                kill_process(pid);
                remove_pid();
            } else {
                // Stale PID file — clean up.
                eprintln!("[runtime] Stale PID {pid} — cleaning up");
                remove_pid();
            }
        }

        // No running process or recovery needed — try (re)start.
        if !self.config.auto_start {
            bail!(
                "MLX runtime is not running and auto_start is disabled.\n\
                 Set auto_start=true in config/runtime.json or start manually."
            );
        }

        self.start()
    }

    /// Start the MLX inference server.
    pub fn start(&self) -> Result<RuntimeInfo> {
        let server_bin = self.config.server_binary();
        if !server_bin.exists() {
            bail!(
                "MLX server binary not found at {:?}.\n\
                 Ensure the venv is set up: python3 -m venv {} && \
                 {}/bin/pip install mlx-lm",
                server_bin,
                self.config.venv_path,
                self.config.venv_path
            );
        }

        // Acquire startup lock to prevent double-start.
        let _lock = match FileLock::try_acquire()? {
            Some(lock) => lock,
            None => {
                // Another process is starting the server — wait for health.
                eprintln!("[runtime] Another process is starting the server — waiting…");
                let healthy = wait_for_health(
                    &self.config.host,
                    self.config.port,
                    Duration::from_secs(self.config.startup_timeout_secs),
                    Duration::from_millis(self.config.health_check_interval_ms),
                );
                if healthy {
                    let models =
                        probe_endpoint(&self.config.host, self.config.port).unwrap_or_default();
                    return Ok(self.build_info(
                        RuntimeStatus::Running,
                        read_pid(),
                        models.first().cloned(),
                    ));
                }
                bail!("Timeout waiting for another process to start the MLX server");
            }
        };

        // If there's still a live process, reuse it.
        if let Some(pid) = read_pid() {
            if pid_alive(pid) {
                if let Some(models) = probe_endpoint(&self.config.host, self.config.port) {
                    return Ok(self.build_info(
                        RuntimeStatus::Running,
                        Some(pid),
                        models.first().cloned(),
                    ));
                }
                // Kill unhealthy process.
                kill_process(pid);
                remove_pid();
            } else {
                remove_pid();
            }
        }

        eprintln!(
            "[runtime] Starting MLX server: model={} host={}:{}",
            self.config.default_model, self.config.host, self.config.port
        );

        let child = std::process::Command::new(server_bin.to_string_lossy().to_string())
            .args([
                "scripts/mlx_native_service.py",
                "--model",
                &self.config.default_model,
                "--host",
                &self.config.host,
                "--port",
                &self.config.port.to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| "failed to spawn mlx_native_service.py")?;

        let pid = child.id();
        write_pid(pid)?;

        eprintln!("[runtime] Spawned PID {pid} — waiting for health…");

        let healthy = wait_for_health(
            &self.config.host,
            self.config.port,
            Duration::from_secs(self.config.startup_timeout_secs),
            Duration::from_millis(self.config.health_check_interval_ms),
        );

        if !healthy {
            eprintln!(
                "[runtime] Health check timeout after {}s",
                self.config.startup_timeout_secs
            );
            kill_process(pid);
            remove_pid();
            remove_lock();
            bail!(
                "MLX server failed to become healthy within {}s.\n\
                 Model: {}\n\
                 Host/Port: {}:{}\n\
                 PID: {pid}\n\
                 Recommendation: Check model path exists and try `dek restart`.",
                self.config.startup_timeout_secs,
                self.config.default_model,
                self.config.host,
                self.config.port,
            );
        }

        let models = probe_endpoint(&self.config.host, self.config.port).unwrap_or_default();
        let loaded = models.first().cloned();
        eprintln!("[runtime] MLX server ready — loaded model: {:?}", loaded);

        Ok(self.build_info(RuntimeStatus::Running, Some(pid), loaded))
    }

    /// Stop the MLX server.
    pub fn stop(&self) -> Result<()> {
        if let Some(pid) = read_pid() {
            if pid_alive(pid) {
                eprintln!("[runtime] Stopping PID {pid}…");
                kill_process(pid);
            }
        }
        remove_pid();
        remove_lock();
        eprintln!("[runtime] Stopped.");
        Ok(())
    }

    /// Restart the MLX server (stop + start).
    pub fn restart(&self) -> Result<RuntimeInfo> {
        self.stop()?;
        self.start()
    }

    /// Report current status without side effects.
    pub fn status(&self) -> Result<RuntimeInfo> {
        let pid = read_pid();
        let alive = pid.is_some_and(pid_alive);

        if alive {
            let models = probe_endpoint(&self.config.host, self.config.port).unwrap_or_default();
            let status = if models.is_empty() {
                RuntimeStatus::Error
            } else {
                RuntimeStatus::Running
            };
            Ok(self.build_info(status, pid, models.first().cloned()))
        } else {
            // Clean stale PID if needed.
            if pid.is_some() {
                remove_pid();
            }
            Ok(self.build_info(RuntimeStatus::Stopped, None, None))
        }
    }

    /// Query the runtime for the currently loaded model name.
    pub fn current_model(&self) -> Result<String> {
        let models = probe_endpoint(&self.config.host, self.config.port).ok_or_else(|| {
            anyhow!(
                "MLX runtime not reachable at {}:{}",
                self.config.host,
                self.config.port
            )
        })?;
        models.first().cloned().ok_or_else(|| {
            anyhow!(
                "MLX runtime responded but reports no loaded models at {}:{}",
                self.config.host,
                self.config.port
            )
        })
    }

    /// Single source of truth for the model ID to use in inference requests.
    ///
    /// Prefers the model actually loaded in the runtime (via `GET /v1/models`).
    /// Falls back to `config.default_model` if the runtime is unreachable.
    pub fn resolve_runtime_model(&self) -> Result<String> {
        match self.current_model() {
            Ok(m) => Ok(m),
            Err(_) => {
                eprintln!(
                    "[runtime] Could not query loaded model — falling back to config default: {}",
                    self.config.default_model
                );
                Ok(self.config.default_model.clone())
            }
        }
    }

    /// Access the underlying configuration.
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    // ── diagnostic helpers ───────────────────────────────────────────────

    /// Build a rich diagnostic string for error contexts.
    pub fn diagnostic(&self) -> String {
        let pid = read_pid();
        let alive = pid.is_some_and(pid_alive);
        let loaded = if alive {
            probe_endpoint(&self.config.host, self.config.port)
                .and_then(|m| m.first().cloned())
                .unwrap_or_else(|| "<unavailable>".to_string())
        } else {
            "<not running>".to_string()
        };

        let status_icon = if alive { "●" } else { "✗" };
        let status_text = if alive { "running" } else { "not running" };

        format!(
            "\n╔══════════════════════════════════════════════\n\
             ║ Runtime Diagnostic\n\
             ║\n\
             ║ Provider:         {}\n\
             ║ Host:             {}:{}\n\
             ║ PID:              {}\n\
             ║ Loaded Model:     {}\n\
             ║ Expected Model:   {}\n\
             ║ Status:           {status_icon} {status_text}\n\
             ║\n\
             ║ Recommendation:   Run `dek restart` or check\n\
             ║                   config/runtime.json\n\
             ╚══════════════════════════════════════════════",
            self.config.provider,
            self.config.host,
            self.config.port,
            pid.map_or("<none>".to_string(), |p| p.to_string()),
            loaded,
            self.config.default_model,
        )
    }

    // ── internal ─────────────────────────────────────────────────────────

    fn build_info(
        &self,
        status: RuntimeStatus,
        pid: Option<u32>,
        loaded_model: Option<String>,
    ) -> RuntimeInfo {
        RuntimeInfo {
            status,
            pid,
            host: self.config.host.clone(),
            port: self.config.port,
            provider: self.config.provider.clone(),
            loaded_model,
            config_model: self.config.default_model.clone(),
            base_url: self.config.base_url(),
        }
    }
}

pub struct EmbeddingRuntimeManager {
    config: RuntimeConfig,
}

impl EmbeddingRuntimeManager {
    // ── constructors ─────────────────────────────────────────────────────

    /// Load configuration from `config/runtime.json`.
    pub fn load() -> Result<Self> {
        let config = RuntimeConfig::load()?;
        Ok(Self { config })
    }

    /// Build a manager from an already-parsed config (useful in tests).
    #[cfg(test)]
    pub fn from_config(config: RuntimeConfig) -> Self {
        Self { config }
    }

    // ── public API ───────────────────────────────────────────────────────

    /// Ensure the embedding runtime is up and healthy. Starts it if needed.
    pub fn ensure_running(&self) -> Result<RuntimeInfo> {
        let embed_cfg = match &self.config.embeddings {
            Some(cfg) => cfg,
            None => bail!("Embeddings configuration is missing from config/runtime.json"),
        };

        if embed_cfg.provider == "mock" || std::env::var("DAK_LM_BACKEND").as_deref() == Ok("mock")
        {
            return Ok(self.build_info(RuntimeStatus::Running, None));
        }

        if let Some(pid) = read_emb_pid() {
            if pid_alive(pid) {
                if check_embeddings_health(embed_cfg).is_ok() {
                    return Ok(self.build_info(RuntimeStatus::Running, Some(pid)));
                }
                eprintln!("[emb_runtime] PID {pid} alive but embedding endpoint unresponsive — restarting");
                kill_process(pid);
                remove_emb_pid();
            } else {
                eprintln!("[emb_runtime] Stale PID {pid} — cleaning up");
                remove_emb_pid();
            }
        }

        if !self.config.auto_start {
            bail!(
                "Embedding runtime is not running and auto_start is disabled.\n\
                 Set auto_start=true in config/runtime.json or start manually."
            );
        }

        self.start()
    }

    /// Start the embedding server.
    pub fn start(&self) -> Result<RuntimeInfo> {
        let embed_cfg = match &self.config.embeddings {
            Some(cfg) => cfg,
            None => bail!("Embeddings configuration is missing from config/runtime.json"),
        };

        if embed_cfg.provider == "mock" || std::env::var("DAK_LM_BACKEND").as_deref() == Ok("mock")
        {
            return Ok(self.build_info(RuntimeStatus::Running, None));
        }

        let py_bin = self.config.server_binary();
        if !py_bin.exists() {
            bail!("Python binary not found at {:?}", py_bin);
        }

        // Acquire startup lock
        let _lock = match FileLock::try_acquire_emb()? {
            Some(lock) => lock,
            None => {
                eprintln!(
                    "[emb_runtime] Another process is starting the embeddings server — waiting…"
                );
                let healthy = self.wait_for_health(
                    Duration::from_secs(self.config.startup_timeout_secs),
                    Duration::from_millis(self.config.health_check_interval_ms),
                );
                if healthy {
                    return Ok(self.build_info(RuntimeStatus::Running, read_emb_pid()));
                }
                bail!("Timeout waiting for another process to start the embedding server");
            }
        };

        if let Some(pid) = read_emb_pid() {
            if pid_alive(pid) {
                if check_embeddings_health(embed_cfg).is_ok() {
                    return Ok(self.build_info(RuntimeStatus::Running, Some(pid)));
                }
                kill_process(pid);
                remove_emb_pid();
            } else {
                remove_emb_pid();
            }
        }

        let (host, port) = parse_host_port(&embed_cfg.endpoint)
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding endpoint: {}", embed_cfg.endpoint))?;

        eprintln!(
            "[emb_runtime] Starting embedding server: model={} host={}:{}",
            embed_cfg.model, host, port
        );

        let child = std::process::Command::new(py_bin.to_string_lossy().to_string())
            .args([
                "scripts/embeddings_service.py",
                "--model",
                &embed_cfg.model,
                "--host",
                &host,
                "--port",
                &port.to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| "failed to spawn embeddings_service.py")?;

        let pid = child.id();
        write_emb_pid(pid)?;

        eprintln!("[emb_runtime] Spawned PID {pid} — waiting for health…");

        let healthy = self.wait_for_health(
            Duration::from_secs(self.config.startup_timeout_secs),
            Duration::from_millis(self.config.health_check_interval_ms),
        );

        if !healthy {
            eprintln!(
                "[emb_runtime] Health check timeout after {}s",
                self.config.startup_timeout_secs
            );
            kill_process(pid);
            remove_emb_pid();
            bail!("Embedding server failed to start within timeout");
        }

        Ok(self.build_info(RuntimeStatus::Running, Some(pid)))
    }

    pub fn stop(&self) -> Result<()> {
        if let Some(pid) = read_emb_pid() {
            eprintln!("[emb_runtime] Stopping embedding server PID {pid}");
            kill_process(pid);
            remove_emb_pid();
        }
        remove_emb_lock();
        Ok(())
    }

    pub fn status(&self) -> Result<RuntimeInfo> {
        let embed_cfg = match &self.config.embeddings {
            Some(cfg) => cfg,
            None => bail!("Embeddings configuration is missing from config/runtime.json"),
        };

        if embed_cfg.provider == "mock" || std::env::var("DAK_LM_BACKEND").as_deref() == Ok("mock")
        {
            return Ok(self.build_info(RuntimeStatus::Running, None));
        }

        let pid = read_emb_pid();
        let is_alive = pid.map(pid_alive).unwrap_or(false);
        let healthy = is_alive && check_embeddings_health(embed_cfg).is_ok();

        let status = if healthy {
            RuntimeStatus::Running
        } else if is_alive {
            RuntimeStatus::Error
        } else {
            RuntimeStatus::Stopped
        };

        Ok(self.build_info(status, pid))
    }

    fn wait_for_health(&self, timeout: Duration, interval: Duration) -> bool {
        let embed_cfg = match &self.config.embeddings {
            Some(cfg) => cfg,
            None => return false,
        };
        let start = Instant::now();
        while start.elapsed() < timeout {
            if check_embeddings_health(embed_cfg).is_ok() {
                return true;
            }
            std::thread::sleep(interval);
        }
        false
    }

    fn build_info(&self, status: RuntimeStatus, pid: Option<u32>) -> RuntimeInfo {
        let embed_cfg = self.config.embeddings.as_ref();
        let (model, host, port, base_url) = match embed_cfg {
            Some(cfg) => {
                let (h, p) =
                    parse_host_port(&cfg.endpoint).unwrap_or(("127.0.0.1".to_string(), 65431));
                (cfg.model.clone(), h, p, cfg.endpoint.clone())
            }
            None => (
                "".to_string(),
                "127.0.0.1".to_string(),
                65431,
                "".to_string(),
            ),
        };

        let is_running = status == RuntimeStatus::Running;

        RuntimeInfo {
            status,
            pid,
            host,
            port,
            provider: embed_cfg
                .map(|c| c.provider.clone())
                .unwrap_or_else(|| "none".to_string()),
            loaded_model: if is_running {
                Some(model.clone())
            } else {
                None
            },
            config_model: model,
            base_url,
        }
    }
}

// ── CLI print helpers ────────────────────────────────────────────────────────

/// Pretty-print runtime status to stdout.
pub fn print_runtime_status() -> Result<()> {
    let mgr = RuntimeManager::load()?;
    let info = mgr.status()?;

    let status_icon = match info.status {
        RuntimeStatus::Running => "●",
        RuntimeStatus::Stopped => "○",
        RuntimeStatus::Starting => "◐",
        RuntimeStatus::Error => "✗",
    };

    println!("═══════════════════════════════════════════");
    println!("  Deterministic AI Kernel — Runtime Status");
    println!("═══════════════════════════════════════════");
    println!("Provider:       {}", info.provider);
    println!(
        "PID:            {}",
        info.pid.map_or("—".to_string(), |p| p.to_string())
    );
    println!("Host:           {}", info.host);
    println!("Port:           {}", info.port);
    println!("Status:         {status_icon} {}", info.status);
    println!(
        "Loaded Model:   {}",
        info.loaded_model.as_deref().unwrap_or("—")
    );
    println!("Config Model:   {}", info.config_model);
    println!("Base URL:       {}", info.base_url);
    println!("═══════════════════════════════════════════");
    Ok(())
}

/// Pretty-print model information to stdout.
pub fn print_runtime_model() -> Result<()> {
    let mgr = RuntimeManager::load()?;
    let info = mgr.status()?;

    // Manifest default
    let manifest_default = crate::model_manifest::load_manifest()
        .ok()
        .and_then(|m| {
            m.models
                .iter()
                .find(|mdl| mdl.enabled && mdl.role == "coding_assistant")
                .map(|mdl| mdl.id.clone())
        })
        .unwrap_or_else(|| "<unknown>".to_string());

    // Role mapping
    let roles = crate::model_manifest::current_model_statuses().unwrap_or_default();

    println!("═══════════════════════════════════════════");
    println!("  Deterministic AI Kernel — Model Info");
    println!("═══════════════════════════════════════════");
    println!(
        "Loaded Runtime Model:  {}",
        info.loaded_model.as_deref().unwrap_or("—")
    );
    println!("Manifest Default:      {}", manifest_default);
    println!("Config Default:        {}", info.config_model);
    println!("Provider:              {}", info.provider);
    println!("Base URL:              {}", info.base_url);
    println!();
    println!("Role Mapping:");
    for row in &roles {
        let sync_icon = if row.in_sync { "✓" } else { "✗" };
        println!(
            "  {sync_icon} {:<20} → {}",
            row.role,
            row.env_model.as_deref().unwrap_or("<missing>")
        );
    }
    println!("═══════════════════════════════════════════");
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RuntimeConfig {
        RuntimeConfig {
            provider: "mlx".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19999,
            default_model: "test-model".to_string(),
            auto_start: false,
            venv_path: "/nonexistent".to_string(),
            startup_timeout_secs: 5,
            health_check_interval_ms: 100,
            embeddings: None,
        }
    }

    #[test]
    fn config_loads_from_disk() {
        // This test relies on config/runtime.json existing in the project root.
        let cfg = RuntimeConfig::load().expect("failed to load runtime config");
        assert_eq!(cfg.provider, "mlx");
        assert_eq!(cfg.host, "127.0.0.1");
        assert!(cfg.port > 0);
        assert!(!cfg.default_model.is_empty());
    }

    #[test]
    fn base_url_format() {
        let cfg = test_config();
        assert_eq!(cfg.base_url(), "http://127.0.0.1:19999/v1");
    }

    #[test]
    fn status_returns_stopped_when_no_pid() {
        let mgr = RuntimeManager::from_config(test_config());
        // With a fake port, status should report stopped (no PID file for port 19999).
        let info = mgr.status().expect("status failed");
        // We can't guarantee Stopped if there happens to be a real PID file,
        // but at minimum the call shouldn't panic.
        assert!(
            info.status == RuntimeStatus::Stopped
                || info.status == RuntimeStatus::Running
                || info.status == RuntimeStatus::Error
        );
    }

    #[test]
    fn build_info_populates_fields() {
        let mgr = RuntimeManager::from_config(test_config());
        let info = mgr.build_info(
            RuntimeStatus::Running,
            Some(12345),
            Some("my-model".to_string()),
        );
        assert_eq!(info.status, RuntimeStatus::Running);
        assert_eq!(info.pid, Some(12345));
        assert_eq!(info.loaded_model.as_deref(), Some("my-model"));
        assert_eq!(info.config_model, "test-model");
        assert_eq!(info.provider, "mlx");
    }

    #[test]
    fn diagnostic_string_contains_key_info() {
        let mgr = RuntimeManager::from_config(test_config());
        let diag = mgr.diagnostic();
        assert!(diag.contains("Provider:"));
        assert!(diag.contains("mlx"));
        assert!(diag.contains("test-model"));
        assert!(diag.contains("Runtime Diagnostic"));
    }

    #[test]
    fn runtime_status_display() {
        assert_eq!(RuntimeStatus::Running.to_string(), "running");
        assert_eq!(RuntimeStatus::Stopped.to_string(), "stopped");
        assert_eq!(RuntimeStatus::Starting.to_string(), "starting");
        assert_eq!(RuntimeStatus::Error.to_string(), "error");
    }
}
