use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Execute a shell command with a HARD timeout.
///
/// Authorization is enforced by the registry gate before this function is
/// ever reached (see tools::registry::execute_tool). The former
/// `is_readonly_command` heuristic no longer exists: it classified
/// interpreters (`python`, `node`, `ruby`) as read-only and was trivially
/// bypassed with `;`, `$()`, backticks, and pipes (audit finding H4).
/// Read-only-ness is a policy decided at the gate, never a string heuristic
/// inside the executor.
///
/// `timeout_secs` is enforced: the child process is killed when the deadline
/// expires (the previous implementation parsed the value and discarded it).
pub async fn shell_execute(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let cmd = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or("missing 'command'")?
        .to_string();
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30)
        .clamp(1, 600);
    let workspace = workspace.to_string();

    let output =
        tokio::task::spawn_blocking(move || run_with_timeout(&cmd, &workspace, timeout_secs))
            .await
            .map_err(|e| format!("spawn error: {e}"))??;

    Ok(output)
}

fn run_with_timeout(
    cmd: &str,
    workspace: &str,
    timeout_secs: u64,
) -> Result<serde_json::Value, String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("exec error: {e}"))?;

    // Drain stdout/stderr on helper threads so the pipes cannot fill up and
    // deadlock the child while we poll for completion.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let out_thread = stdout_pipe.map(|mut p| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = p.read_to_string(&mut s);
            s
        })
    });
    let err_thread = stderr_pipe.map(|mut p| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = p.read_to_string(&mut s);
            s
        })
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait().map_err(|e| format!("wait error: {e}"))? {
            Some(status) => break status,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "command timed out after {}s and was killed",
                        timeout_secs
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let stdout = out_thread
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = err_thread
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let exit_code = status.code().unwrap_or(-1);

    Ok(serde_json::json!({
        "command": cmd,
        "exit_code": exit_code,
        "stdout": stdout,
        "stderr": stderr,
        "success": status.success(),
        "timeout_secs": timeout_secs,
    }))
}
