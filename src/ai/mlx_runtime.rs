use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::kernel_types::{AIRequest, AIResponse};

#[derive(Debug)]
pub struct MlxRuntime {
    config: Arc<MlxRuntimeConfig>,
    daemon: Arc<Mutex<Option<BridgeDaemon>>>,
}

#[derive(Debug, Clone)]
struct MlxRuntimeConfig {
    model_path: PathBuf,
    python_bin: String,
    bridge_script: PathBuf,
    socket_path: PathBuf,
}

#[derive(Debug)]
struct BridgeDaemon {
    child: Child,
}

#[derive(Debug, Serialize)]
struct BridgeRequest<'a> {
    request_id: &'a str,
    workflow_id: &'a str,
    prompt: &'a str,
    model_id: &'a Option<String>,
    seed: &'a Option<u64>,
}

#[derive(Debug, Deserialize)]
struct BridgeResponse {
    request_id: Option<String>,
    output_text: Option<String>,
    finished: bool,
    error: Option<String>,
}

impl MlxRuntime {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/denissmoliakov".to_string());
        let pid = std::process::id();

        Self {
            config: Arc::new(MlxRuntimeConfig {
                model_path: PathBuf::from(
                    std::env::var("MLX_MODEL_PATH")
                        .unwrap_or_else(|_| "~/Models/gemma4-reasoning".to_string())
                        .replace("~", &home),
                ),
                python_bin: std::env::var("MLX_PYTHON_BIN")
                    .unwrap_or_else(|_| "python3".to_string()),
                bridge_script: PathBuf::from(
                    std::env::var("MLX_BRIDGE_SCRIPT")
                        .unwrap_or_else(|_| "scripts/mlx_bridge.py".to_string()),
                ),
                socket_path: PathBuf::from(
                    std::env::var("MLX_BRIDGE_SOCKET").unwrap_or_else(|_| {
                        format!("/tmp/deterministic_ai_kernel_mlx_{}.sock", pid)
                    }),
                ),
            }),
            daemon: Arc::new(Mutex::new(None)),
        }
    }

    pub fn health_check(&self) -> Result<()> {
        if !self.config.model_path.exists() {
            return Err(anyhow!(
                "MLX model path does not exist: {}",
                self.config.model_path.display()
            ));
        }

        if !self.config.model_path.is_dir() {
            return Err(anyhow!(
                "MLX model path is not a directory: {}",
                self.config.model_path.display()
            ));
        }

        let config_json = self.config.model_path.join("config.json");
        if !config_json.exists() {
            return Err(anyhow!(
                "MLX model config.json missing: {}",
                config_json.display()
            ));
        }

        if !self.config.bridge_script.exists() {
            return Err(anyhow!(
                "MLX bridge script missing: {}",
                self.config.bridge_script.display()
            ));
        }

        let python_check = Command::new(&self.config.python_bin)
            .arg("-c")
            .arg("import importlib.util,sys; sys.exit(0 if importlib.util.find_spec('mlx_lm') else 1)")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| {
                format!(
                    "failed to execute python for mlx_lm check via {}",
                    self.config.python_bin
                )
            })?;

        if !python_check.success() {
            return Err(anyhow!(
                "mlx_lm is not installed for {}",
                self.config.python_bin
            ));
        }

        Ok(())
    }

    pub fn generate(&self, request: &AIRequest) -> Result<AIResponse> {
        self.health_check()?;
        self.ensure_daemon()?;

        let payload = serde_json::to_string(&BridgeRequest {
            request_id: &request.request_id,
            workflow_id: &request.workflow_id,
            prompt: &request.prompt,
            model_id: &request.model_id,
            seed: &request.seed,
        })
        .context("failed to serialize MLX bridge request")?;

        let mut stream = UnixStream::connect(&self.config.socket_path).with_context(|| {
            format!(
                "failed to connect to MLX bridge socket {}",
                self.config.socket_path.display()
            )
        })?;

        stream
            .write_all(payload.as_bytes())
            .context("failed to write MLX bridge request payload")?;
        stream
            .write_all(b"\n")
            .context("failed to terminate MLX bridge request payload")?;
        stream
            .flush()
            .context("failed to flush MLX bridge request")?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .context("failed to read MLX bridge response")?;

        let bridge: BridgeResponse = serde_json::from_str(line.trim())
            .context("failed to decode MLX bridge response JSON")?;

        if let Some(error) = bridge.error {
            return Err(anyhow!("MLX bridge daemon failed: {}", error));
        }

        Ok(AIResponse {
            request_id: bridge
                .request_id
                .unwrap_or_else(|| request.request_id.clone()),
            output_text: bridge.output_text,
            output_artifact_ids: Vec::new(),
            finished: bridge.finished,
        })
    }

    fn ensure_daemon(&self) -> Result<()> {
        let mut guard = self
            .daemon
            .lock()
            .map_err(|_| anyhow!("failed to lock MLX daemon state"))?;

        let needs_start = match guard.as_mut() {
            Some(existing) => existing
                .child
                .try_wait()
                .context("failed to probe MLX daemon status")?
                .is_some(),
            None => true,
        };

        if !needs_start {
            return Ok(());
        }

        *guard = Some(self.spawn_daemon()?);
        Ok(())
    }

    fn spawn_daemon(&self) -> Result<BridgeDaemon> {
        if self.config.socket_path.exists() {
            std::fs::remove_file(&self.config.socket_path).with_context(|| {
                format!(
                    "failed to remove stale MLX bridge socket {}",
                    self.config.socket_path.display()
                )
            })?;
        }

        let mut child = Command::new(&self.config.python_bin)
            .arg(&self.config.bridge_script)
            .arg("--model")
            .arg(&self.config.model_path)
            .arg("--daemon")
            .arg("--socket-path")
            .arg(&self.config.socket_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn MLX bridge daemon: {} {}",
                    self.config.python_bin,
                    self.config.bridge_script.display()
                )
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("failed to acquire stdout for MLX bridge daemon"))?;

        self.wait_for_ready(stdout, &mut child)?;
        Ok(BridgeDaemon { child })
    }

    fn wait_for_ready(&self, stdout: ChildStdout, child: &mut Child) -> Result<()> {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .context("failed to read MLX daemon readiness line")?;

        if bytes == 0 {
            let stderr = child
                .stderr
                .take()
                .map(read_stderr)
                .transpose()?
                .unwrap_or_else(|| "<no stderr>".to_string());
            return Err(anyhow!(
                "MLX bridge daemon exited before readiness: {}",
                stderr
            ));
        }

        let ready: serde_json::Value =
            serde_json::from_str(line.trim()).context("invalid MLX daemon readiness JSON")?;

        if ready.get("status").and_then(|v| v.as_str()) != Some("ready") {
            return Err(anyhow!(
                "unexpected MLX daemon readiness payload: {}",
                line.trim()
            ));
        }

        Ok(())
    }
}

impl Default for MlxRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MlxRuntime {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            daemon: Arc::clone(&self.daemon),
        }
    }
}

impl Drop for MlxRuntime {
    fn drop(&mut self) {
        if Arc::strong_count(&self.daemon) != 1 {
            return;
        }

        if let Ok(mut guard) = self.daemon.lock() {
            if let Some(mut daemon) = guard.take() {
                let _ = daemon.child.kill();
                let _ = daemon.child.wait();
            }
        }

        if self.config.socket_path.exists() {
            let _ = std::fs::remove_file(&self.config.socket_path);
        }
    }
}

fn read_stderr(stderr: std::process::ChildStderr) -> Result<String> {
    let mut reader = BufReader::new(stderr);
    let mut buf = String::new();
    reader
        .read_to_string(&mut buf)
        .context("failed to read MLX daemon stderr")?;
    Ok(buf.trim().to_string())
}
