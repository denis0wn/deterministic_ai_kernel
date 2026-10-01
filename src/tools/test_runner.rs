//! Real authorized test execution (P3).
//!
//! RunTests is no longer LLM-simulated. The kernel — never the model —
//! decides WHICH command runs (workspace-derived allowlist) and the result
//! comes from actually executing it inside the workspace, with a hard
//! timeout, captured output, and a kernel-owned `test_report_v1` evidence
//! record. The model may propose validation text in a PatchV1, but no
//! LLM-supplied string is ever executed or interpreted as a command.
//!
//! Security model:
//! - command identity comes from `derive_test_command` (kernel inspection of
//!   the workspace), never from LLM output, never from tool arguments;
//! - execution is a direct argv spawn (`Command::new(prog).args(argv)`) —
//!   no `sh -c`, no shell parsing, no user/LLM command strings;
//! - cwd is the canonicalized workspace; the allowlist only produces
//!   standard test runners (cargo test / python3 -m pytest / python3
//!   executing a test file through the kernel-owned harness below);
//! - timeout is enforced with kill-on-deadline;
//! - authorization is enforced by `tools::registry::execute_tool` before
//!   this handler is reached (confirmation-gated like other mutating tools).

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const TEST_REPORT_VERSION: &str = "test_report_v1";
pub const DEFAULT_TEST_TIMEOUT_SECS: u64 = 120;
const OUTPUT_TAIL_CHARS: usize = 2000;

/// Kernel-owned harness for `python_test_file` (CD-1 fix). Fed to python3
/// via stdin (`python3 - <file>`) — a compile-time constant, never LLM
/// output. It executes the test file as `__main__` (preserving
/// script-is-test semantics: top-level asserts, `if __name__` guards,
/// sys.exit) and THEN invokes every module-level `test_*` function, so
/// pytest-style files can no longer pass vacuously just because nothing
/// calls their test functions. Any uncaught exception => non-zero exit =>
/// `tests_failed`. Fixture/parametrize-style tests require a real pytest
/// configuration marker (pytest.ini/pyproject.toml/...) and route to
/// `python_pytest` instead.
const PYTHON_TEST_FILE_HARNESS: &str = r#"
import inspect
import json
import os
import runpy
import shutil
import sys
import traceback

# Stale bytecode cache can shadow freshly patched sources (same-second
# writes with identical byte length keep a pyc "valid") — fatal for the
# feedback loop, which re-runs tests seconds after a re-patch. Purge
# caches under the workspace and do not write new ones.
sys.dont_write_bytecode = True
for _root, _dirs, _files in os.walk("."):
    for _d in [d for d in _dirs if d == "__pycache__"]:
        shutil.rmtree(os.path.join(_root, _d), ignore_errors=True)


def _kernel_test_harness():
    if len(sys.argv) < 2:
        print("kernel test harness: missing test file argument", file=sys.stderr)
        return 2
    path = sys.argv[1]
    try:
        module_globals = runpy.run_path(path, run_name="__main__")
    except SystemExit as exc:
        code = exc.code
        if code is None or code == 0:
            return 0
        return code if isinstance(code, int) else 1
    except BaseException:
        traceback.print_exc()
        return 1

    failures = []
    for name in sorted(module_globals):
        fn = module_globals[name]
        if not name.startswith("test_") or not inspect.isfunction(fn):
            continue
        try:
            fn()
        except BaseException:
            failures.append(name)
            traceback.print_exc()
    if failures:
        # Machine-readable marker for the kernel (C0): names only, no
        # messages, no values. Printed AFTER all test output so a test file
        # printing a fake marker line cannot shadow this record — the
        # kernel parses the LAST marker line.
        print("DAK_TEST_FAILURES_V1 " + json.dumps(failures), file=sys.stderr)
        print(
            "kernel test harness: FAILED " + ", ".join(failures),
            file=sys.stderr,
        )
        return 1
    print("kernel test harness: OK")
    return 0


sys.exit(_kernel_test_harness())
"#;

/// Marker prefix the harness prints (stderr) with the JSON array of failing
/// test names. See the harness comment: the last marker line is canonical.
const FAILURES_MARKER: &str = "DAK_TEST_FAILURES_V1 ";

/// Extract failing test names from harness stderr (C0). Fail-closed to an
/// empty list on any anomaly: missing marker, malformed JSON, non-identifier
/// or overlong names. Names are the only verifier output the feedback path
/// may consume — never messages or values.
fn parse_failures(stderr: &str) -> Vec<String> {
    let Some(line) = stderr
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(FAILURES_MARKER))
    else {
        return Vec::new();
    };
    let Ok(names) = serde_json::from_str::<Vec<String>>(line) else {
        return Vec::new();
    };
    names
        .into_iter()
        .filter(|n| {
            !n.is_empty() && n.len() <= 200 && n.chars().all(|c| c.is_alphanumeric() || c == '_')
        })
        .take(64)
        .collect()
}

/// P4-B kernel-owned outcome taxonomy. Classification is computed by the
/// kernel from argv/spawn result/exit status/timeout/runner semantics —
/// never by the LLM. `passed` is true ONLY for `tests_passed`.
pub mod outcome {
    pub const TESTS_PASSED: &str = "tests_passed";
    pub const TESTS_FAILED: &str = "tests_failed";
    pub const TIMEOUT: &str = "timeout";
    pub const SPAWN_FAILED: &str = "spawn_failed";
    pub const COMMAND_NOT_FOUND: &str = "command_not_found";
    pub const INFRASTRUCTURE_ERROR: &str = "infrastructure_error";
}

/// Kernel-owned classification of a test command outcome.
///
/// Runner semantics (deterministic, no LLM/NLP):
/// - cargo_test: exit 101 WITH a "test result:" line => tests actually ran
///   and failed; exit!=0 WITHOUT it => infrastructure_error (e.g. compile
///   failure) — "exit != 0" is NOT assumed to mean test failure.
/// - python_pytest: exit 1 => tests_failed; exit 2/3/4/5 (interrupt,
///   internal, usage, nothing collected) => infrastructure_error.
/// - python_test_file: the file is executed as `__main__` (script-is-test
///   semantics) AND every module-level test_* function is invoked by the
///   kernel-owned harness; any non-zero exit is a test failure (stderr
///   captured as evidence).
pub fn classify_outcome(
    command_id: &str,
    spawn_error_kind: Option<std::io::ErrorKind>,
    timed_out: bool,
    exit_code: i32,
    stdout: &str,
) -> &'static str {
    use outcome::*;
    if let Some(kind) = spawn_error_kind {
        return match kind {
            std::io::ErrorKind::NotFound => COMMAND_NOT_FOUND,
            _ => SPAWN_FAILED,
        };
    }
    if timed_out {
        return TIMEOUT;
    }
    if exit_code == 0 {
        // Security (found in the Layer-2 review): the kernel harness runs
        // workspace code IN-PROCESS via runpy, and a model-patched module
        // can `os._exit(0)` at import time — killing the harness before any
        // test ran, with a success exit code. For the harness runner,
        // "passed" additionally requires the harness's own OK line, printed
        // only after every test_* function returned. Deliberate forgery of
        // the marker string by in-process code is the M-2 isolation
        // problem, out of scope here.
        if command_id == "python_test_file"
            && !stdout.lines().any(|l| l == "kernel test harness: OK")
        {
            return TESTS_FAILED;
        }
        return TESTS_PASSED;
    }
    match command_id {
        "cargo_test" => {
            if stdout.contains("test result:") {
                TESTS_FAILED
            } else {
                INFRASTRUCTURE_ERROR
            }
        }
        "python_pytest" => {
            if exit_code == 1 {
                TESTS_FAILED
            } else {
                INFRASTRUCTURE_ERROR
            }
        }
        "python_test_file" => TESTS_FAILED,
        _ => INFRASTRUCTURE_ERROR,
    }
}

/// Kernel-owned structured test evidence. `passed` is derived ONLY from the
/// real exit status of the executed command — no LLM opinion can set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestReportV1 {
    pub version: String,
    pub command_id: String,
    pub argv: Vec<String>,
    pub exit_code: i32,
    pub passed: bool,
    pub timed_out: bool,
    /// P4-B: kernel-owned taxonomy (see `outcome` module).
    pub classification: String,
    pub stdout_tail: String,
    pub stderr_tail: String,
    /// C0: failing test names for the `python_test_file` runner (empty for
    /// other runners and for module-body failures, where no test ran).
    /// Names only — never messages or values. Default keeps reports
    /// persisted before this field readable.
    #[serde(default)]
    pub failures: Vec<String>,
    /// Which sandbox confined this run ("seatbelt", "bwrap", "none").
    /// Evidence honesty: an unsandboxed run says "none" — never overclaimed.
    #[serde(default = "default_sandbox_backend")]
    pub sandbox_backend: String,
    pub duration_ms: u64,
    pub workspace: String,
    pub captured_unix: u64,
}

/// Allowlisted test command derived from KERNEL inspection of the workspace.
/// LLM output is never consulted here. `stdin_program`, when present, is a
/// kernel-owned constant fed to the child's stdin (never LLM output).
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedTestCommand {
    pub command_id: String,
    pub program: String,
    pub argv: Vec<String>,
    pub stdin_program: Option<&'static str>,
}

/// Inspect the workspace and pick the test runner from a fixed allowlist.
/// Deterministic order: cargo > pytest-config > first test_*.py file.
/// Fails closed when nothing allowlisted is present.
pub fn derive_test_command(workspace: &Path) -> Result<DerivedTestCommand, String> {
    if !workspace.is_dir() {
        return Err(format!(
            "workspace is not a directory: {}",
            workspace.display()
        ));
    }

    // Rust project: cargo test.
    if workspace.join("Cargo.toml").is_file() {
        return Ok(DerivedTestCommand {
            command_id: "cargo_test".to_string(),
            program: "cargo".to_string(),
            argv: vec![
                "test".to_string(),
                "--color".to_string(),
                "never".to_string(),
            ],
            stdin_program: None,
        });
    }

    // Python project with an explicit pytest configuration.
    for marker in ["pytest.ini", "pyproject.toml", "setup.py", "tox.ini"] {
        if workspace.join(marker).is_file() {
            return Ok(DerivedTestCommand {
                command_id: "python_pytest".to_string(),
                program: "python3".to_string(),
                argv: vec!["-m".to_string(), "pytest".to_string(), "-q".to_string()],
                stdin_program: None,
            });
        }
    }

    // Plain python test files: run the first one alphabetically through the
    // kernel-owned harness (`python3 - <file>`, harness on stdin). The
    // harness executes the file as `__main__` and then invokes every
    // module-level test_* function, so pytest-style files cannot pass
    // vacuously just because nothing calls their test functions (CD-1:
    // bare `python3 <file>` exited 0 on def test_* files and produced a
    // false tests_passed).
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(workspace)
        .map_err(|e| format!("cannot read workspace: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().map(|x| x == "py").unwrap_or(false)
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("test_") || n.ends_with("_test.py"))
                    .unwrap_or(false)
        })
        .collect();
    candidates.sort();
    if let Some(first) = candidates.first() {
        let rel = first
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        return Ok(DerivedTestCommand {
            command_id: "python_test_file".to_string(),
            program: "python3".to_string(),
            argv: vec!["-".to_string(), rel],
            stdin_program: Some(PYTHON_TEST_FILE_HARNESS),
        });
    }

    Err(format!(
        "no allowlisted test command for workspace {}",
        workspace.display()
    ))
}

/// M-2: Seatbelt profile for the `python_test_file` runner (macOS only).
/// The harness executes workspace code in-process; this profile bounds the
/// blast radius of model-written code: no network, no writes outside the
/// canonicalized workspace, no reads of common credential stores. The
/// workspace path is passed canonicalized because Seatbelt subpath rules
/// match literally (and /tmp is a symlink on macOS).
///
/// Out of scope: python_pytest and cargo_test runners (they need broader
/// filesystem access; documented in LAYER2_SECURITY_REVIEW.md F4). The
/// verdict-integrity ceiling (in-process forgery of the OK marker) is
/// unaffected by sandboxing — that is the residual documented there.
const SANDBOX_PROFILE: &str = "(version 1)\
    (allow default)\
    (deny network*)\
    (deny file-write*)\
    (allow file-write* (subpath (param \"DAK_WS\")))\
    (deny file-read* (subpath (param \"DAK_SSH\")))\
    (deny file-read* (subpath (param \"DAK_AWS\")))\
    (deny file-read* (subpath (param \"DAK_GNUPG\")))\
    (deny file-read* (subpath (param \"DAK_KUBE\")))\
    (deny file-read* (subpath (param \"DAK_NETRC\")))";

/// Which confinement actually wrapped the harness run — recorded in the
/// report so evidence never overclaims. "none" is honest: an unsandboxed
/// run says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxBackend {
    Seatbelt,
    Bwrap,
    None,
}

impl SandboxBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxBackend::Seatbelt => "seatbelt",
            SandboxBackend::Bwrap => "bwrap",
            SandboxBackend::None => "none",
        }
    }
}

/// DAK_TEST_SANDBOX=off|0|false disables sandboxing (operator escape hatch).
pub fn sandbox_enabled() -> bool {
    !matches!(
        std::env::var("DAK_TEST_SANDBOX").ok().as_deref(),
        Some("off") | Some("0") | Some("false")
    )
}

fn sandbox_exec_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| Path::new("/usr/bin/sandbox-exec").exists())
}

fn bwrap_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        // bwrap may live in /usr/bin or elsewhere on PATH
        std::process::Command::new("bwrap")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// Wrap the python_test_file harness invocation in a sandbox (M-2).
/// macOS: sandbox-exec/Seatbelt. Linux: bubblewrap (ro-bind /, rw workspace,
/// --unshare-net, credential dirs masked). Returns (program, argv, backend).
/// Other runners, missing tooling, and the env kill switch pass through as
/// backend "none" — which the report records honestly.
fn maybe_sandbox(
    command_id: &str,
    program: &str,
    argv: &[String],
    ws: &Path,
) -> (String, Vec<String>, SandboxBackend) {
    if command_id != "python_test_file" || !sandbox_enabled() {
        return (program.to_string(), argv.to_vec(), SandboxBackend::None);
    }
    if cfg!(target_os = "macos") && sandbox_exec_available() {
        let home = std::env::var("HOME").unwrap_or_default();
        let mut wrapped: Vec<String> = vec![
            "-D".into(),
            format!("DAK_WS={}", ws.display()),
            "-D".into(),
            format!("DAK_SSH={home}/.ssh"),
            "-D".into(),
            format!("DAK_AWS={home}/.aws"),
            "-D".into(),
            format!("DAK_GNUPG={home}/.gnupg"),
            "-D".into(),
            format!("DAK_KUBE={home}/.kube"),
            "-D".into(),
            format!("DAK_NETRC={home}/.netrc"),
            "-p".into(),
            SANDBOX_PROFILE.to_string(),
            program.to_string(),
        ];
        wrapped.extend(argv.iter().cloned());
        return (
            "/usr/bin/sandbox-exec".to_string(),
            wrapped,
            SandboxBackend::Seatbelt,
        );
    }
    if cfg!(target_os = "linux") && bwrap_available() {
        // Read-only system, writable workspace, no network, credential
        // stores masked with empty tmpfs (/dev/null for the file ones).
        let home = std::env::var("HOME").unwrap_or_default();
        let mut wrapped: Vec<String> = vec![
            "--ro-bind".into(),
            "/".into(),
            "/".into(),
            "--bind".into(),
            ws.to_string_lossy().into_owned(),
            ws.to_string_lossy().into_owned(),
            "--unshare-net".into(),
            "--dev-bind".into(),
            "/dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
        ];
        for dir in [".ssh", ".aws", ".gnupg", ".kube"] {
            wrapped.extend(["--tmpfs".into(), format!("{home}/{dir}")]);
        }
        wrapped.extend([
            "--ro-bind".into(),
            "/dev/null".into(),
            format!("{home}/.netrc"),
            "--chdir".into(),
            ws.to_string_lossy().into_owned(),
            "--".into(),
            program.to_string(),
        ]);
        wrapped.extend(argv.iter().cloned());
        return ("bwrap".to_string(), wrapped, SandboxBackend::Bwrap);
    }
    (program.to_string(), argv.to_vec(), SandboxBackend::None)
}

fn default_sandbox_backend() -> String {
    "none".to_string()
}

fn tail(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars()
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

/// Execute the derived test command for `workspace` with a hard timeout.
/// Direct argv spawn: no shell is involved at any point.
///
/// P4-B: spawn failures and missing executables produce a report carrying
/// the kernel-owned classification (spawn_failed / command_not_found)
/// instead of a bare error, so the evidence taxonomy is observable even
/// when nothing could be executed. Workspace/allowlist resolution errors
/// still return Err (no command identity exists yet).
pub fn run_tests(workspace: &str, timeout_secs: u64) -> Result<TestReportV1, String> {
    let ws = Path::new(workspace)
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize workspace '{workspace}': {e}"))?;
    let derived = derive_test_command(&ws)?;
    let timeout_secs = timeout_secs.clamp(1, 600);

    let started = Instant::now();
    let needs_stdin = derived.stdin_program.is_some();
    // M-2: on macOS the python_test_file harness runs under sandbox-exec
    // (network denied, writes confined to the workspace). The report's argv
    // records the actual wrapped command — evidence stays honest.
    let (program, run_argv, sandbox_backend) =
        maybe_sandbox(&derived.command_id, &derived.program, &derived.argv, &ws);
    let spawn_result = Command::new(&program)
        .args(&run_argv)
        .current_dir(&ws)
        .stdin(if needs_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let argv: Vec<String> = std::iter::once(program).chain(run_argv).collect();
    let report_tail = |stdout: String,
                       stderr: String,
                       exit_code: i32,
                       timed_out: bool,
                       classification: &'static str|
     -> TestReportV1 {
        let failures = if derived.command_id == "python_test_file" {
            parse_failures(&stderr)
        } else {
            Vec::new()
        };
        TestReportV1 {
            version: TEST_REPORT_VERSION.to_string(),
            command_id: derived.command_id.clone(),
            argv: argv.clone(),
            exit_code,
            passed: classification == outcome::TESTS_PASSED,
            timed_out,
            classification: classification.to_string(),
            stdout_tail: tail(&stdout, OUTPUT_TAIL_CHARS),
            stderr_tail: tail(&stderr, OUTPUT_TAIL_CHARS),
            failures,
            sandbox_backend: sandbox_backend.as_str().to_string(),
            duration_ms: started.elapsed().as_millis() as u64,
            workspace: ws.to_string_lossy().into_owned(),
            captured_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    };

    let mut child = match spawn_result {
        Ok(c) => c,
        Err(e) => {
            let classification =
                classify_outcome(&derived.command_id, Some(e.kind()), false, -1, "");
            return Ok(report_tail(
                String::new(),
                format!("spawn error: {e}"),
                -1,
                false,
                classification,
            ));
        }
    };

    // Feed the kernel-owned harness to the child before polling. The write
    // fits in the pipe buffer; a write error only occurs if the child has
    // already exited, in which case its exit status classifies the outcome.
    if let Some(program) = derived.stdin_program {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(program.as_bytes());
        }
    }

    // Drain pipes on helper threads so the child cannot deadlock on full
    // pipe buffers while we poll for completion.
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

    let deadline = started + Duration::from_secs(timeout_secs);
    let mut timed_out = false;
    let status: Option<std::process::ExitStatus> = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("test process wait error: {e}")),
        }
    };

    let stdout = out_thread
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = err_thread
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let exit_code = status.and_then(|s| s.code()).unwrap_or(-1);
    let classification = classify_outcome(&derived.command_id, None, timed_out, exit_code, &stdout);

    Ok(report_tail(
        stdout,
        stderr,
        exit_code,
        timed_out,
        classification,
    ))
}

/// Registry tool handler (`run_tests_v1`). Arguments may only tune the
/// timeout; the command itself is kernel-derived. Authorization is enforced
/// by the registry gate before this is reached.
pub async fn run_tests_tool(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TEST_TIMEOUT_SECS);
    let workspace = workspace.to_string();
    let report = tokio::task::spawn_blocking(move || run_tests(&workspace, timeout_secs))
        .await
        .map_err(|e| format!("spawn error: {e}"))??;
    serde_json::to_value(&report).map_err(|e| format!("report serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dak_p3_{name}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn derive_prefers_cargo() {
        let ws = unique_dir("derive_cargo");
        std::fs::write(ws.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        // Even with python test files present, cargo wins (deterministic order).
        std::fs::write(ws.join("test_x.py"), "assert True\n").unwrap();
        let d = derive_test_command(&ws).unwrap();
        assert_eq!(d.command_id, "cargo_test");
        assert_eq!(d.program, "cargo");
        assert_eq!(d.argv, vec!["test", "--color", "never"]);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn derive_picks_pytest_config() {
        let ws = unique_dir("derive_pytest");
        std::fs::write(ws.join("pytest.ini"), "[pytest]\n").unwrap();
        let d = derive_test_command(&ws).unwrap();
        assert_eq!(d.command_id, "python_pytest");
        assert_eq!(d.argv, vec!["-m", "pytest", "-q"]);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn derive_picks_first_test_file_alphabetically() {
        let ws = unique_dir("derive_file");
        std::fs::write(ws.join("test_b.py"), "assert True\n").unwrap();
        std::fs::write(ws.join("test_a.py"), "assert True\n").unwrap();
        std::fs::write(ws.join("calc.py"), "x = 1\n").unwrap();
        let d = derive_test_command(&ws).unwrap();
        assert_eq!(d.command_id, "python_test_file");
        // `python3 - <file>`: kernel-owned harness arrives on stdin.
        assert_eq!(d.argv, vec!["-", "test_a.py"]);
        assert!(d.stdin_program.is_some(), "harness must ride stdin");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn derive_fails_closed_for_empty_workspace() {
        let ws = unique_dir("derive_empty");
        let err = derive_test_command(&ws).unwrap_err();
        assert!(err.contains("no allowlisted test command"), "{err}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn derive_rejects_nonexistent_workspace() {
        let err = derive_test_command(Path::new("/tmp/dak_p3_no_such_dir_xyz")).unwrap_err();
        assert!(err.contains("not a directory"), "{err}");
    }

    #[test]
    fn run_tests_real_passing_python() {
        let ws = unique_dir("run_pass");
        std::fs::write(
            ws.join("calc.py"),
            "def multiply(a, b):\n    return a * b\n",
        )
        .unwrap();
        std::fs::write(
            ws.join("test_calc.py"),
            "from calc import multiply\nassert multiply(2, 3) == 6\nprint('TEST_OK')\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).expect("run");
        assert!(report.passed);
        assert!(!report.timed_out);
        assert_eq!(report.exit_code, 0);
        assert_eq!(report.command_id, "python_test_file");
        assert!(report.stdout_tail.contains("TEST_OK"));
        assert_eq!(report.version, TEST_REPORT_VERSION);
        assert!(report
            .workspace
            .ends_with(ws.file_name().unwrap().to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn run_tests_real_failing_python() {
        let ws = unique_dir("run_fail");
        std::fs::write(
            ws.join("calc.py"),
            "def multiply(a, b):\n    return a + b\n",
        )
        .unwrap();
        std::fs::write(
            ws.join("test_calc.py"),
            "from calc import multiply\nassert multiply(2, 3) == 6\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).expect("run");
        assert!(!report.passed);
        assert_ne!(report.exit_code, 0);
        assert!(
            report.stderr_tail.contains("AssertionError"),
            "stderr_tail: {}",
            report.stderr_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    // ── CD-1 regression: pytest-style files must not pass vacuously ────
    //
    // Acceptance F3 proved the defect: a file containing only `def test_*`
    // functions ran as bare `python3 <file>`, which merely defined the
    // functions and exited 0 => false tests_passed => false validation PASS
    // for a change whose tests objectively fail.

    #[test]
    fn pytest_style_failing_tests_cannot_pass_vacuously() {
        let ws = unique_dir("ptf_style_fail");
        std::fs::write(
            ws.join("calc.py"),
            "def multiply(a, b):\n    return a * b\n",
        )
        .unwrap();
        std::fs::write(
            ws.join("test_calc.py"),
            "from calc import multiply\n\n\ndef test_multiply():\n    assert multiply(2, 3) == 999\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).expect("run");
        assert!(!report.passed, "vacuous pass is the CD-1 defect");
        assert_ne!(report.exit_code, 0);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        assert!(
            report.stderr_tail.contains("AssertionError"),
            "stderr_tail: {}",
            report.stderr_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn pytest_style_passing_tests_pass() {
        let ws = unique_dir("ptf_style_pass");
        std::fs::write(
            ws.join("calc.py"),
            "def multiply(a, b):\n    return a * b\n",
        )
        .unwrap();
        std::fs::write(
            ws.join("test_calc.py"),
            "from calc import multiply\n\n\ndef test_multiply():\n    assert multiply(2, 3) == 6\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).expect("run");
        assert!(report.passed);
        assert_eq!(report.exit_code, 0);
        assert_eq!(report.classification, outcome::TESTS_PASSED);
        assert!(
            report.stdout_tail.contains("kernel test harness: OK"),
            "stdout_tail: {}",
            report.stdout_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn module_level_statements_do_not_mask_uninvoked_failing_tests() {
        // Top-level statements alone used to make bare `python3 <file>`
        // exit 0; the harness must still invoke the failing test function.
        let ws = unique_dir("ptf_const");
        std::fs::write(
            ws.join("test_x.py"),
            "CONST = 1\n\n\ndef test_bad():\n    assert False\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).expect("run");
        assert!(!report.passed);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn main_guarded_script_semantics_preserved() {
        // Script-is-test semantics survive the harness: an `if __name__`
        // guarded assertion still executes (run_name="__main__").
        let ws = unique_dir("ptf_guard");
        std::fs::write(
            ws.join("test_guard.py"),
            "if __name__ == \"__main__\":\n    assert 1 == 2\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).expect("run");
        assert!(!report.passed);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn run_tests_timeout_kills_and_fails() {
        let ws = unique_dir("run_timeout");
        std::fs::write(ws.join("test_slow.py"), "import time\ntime.sleep(30)\n").unwrap();
        let started = Instant::now();
        let report = run_tests(ws.to_str().unwrap(), 1).expect("run");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "must kill fast"
        );
        assert!(!report.passed);
        assert!(report.timed_out);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn run_tests_unavailable_command_fails_closed() {
        let ws = unique_dir("run_nocmd");
        let err = run_tests(ws.to_str().unwrap(), 5).unwrap_err();
        assert!(err.contains("no allowlisted test command"), "{err}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn argv_never_contains_shell() {
        // The allowlist must never route through a shell interpreter.
        for ws_maker in [
            |ws: &Path| std::fs::write(ws.join("Cargo.toml"), "").unwrap(),
            |ws: &Path| std::fs::write(ws.join("pytest.ini"), "").unwrap(),
            |ws: &Path| std::fs::write(ws.join("test_a.py"), "").unwrap(),
        ] {
            let ws = unique_dir("argv_shell");
            ws_maker(&ws);
            let d = derive_test_command(&ws).unwrap();
            assert_ne!(d.program, "sh");
            assert_ne!(d.program, "bash");
            assert!(d.argv.iter().all(|a| a != "-c"), "no shell -c allowed");
            let _ = std::fs::remove_dir_all(&ws);
        }
    }

    #[test]
    fn report_serializes_to_test_report_v1_json() {
        let ws = unique_dir("serde");
        std::fs::write(ws.join("test_ok.py"), "assert True\n").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        let v = serde_json::to_value(&report).unwrap();
        assert_eq!(v["version"], "test_report_v1");
        assert_eq!(v["passed"], true);
        assert_eq!(v["classification"], "tests_passed");
        assert!(v["argv"].is_array());
        let back: TestReportV1 = serde_json::from_value(v).unwrap();
        assert_eq!(back, report);
        let _ = std::fs::remove_dir_all(&ws);
    }

    // ── P4-B: kernel-owned outcome taxonomy ─────────────────────────────

    #[test]
    fn classify_spawn_errors_without_execution() {
        assert_eq!(
            classify_outcome(
                "cargo_test",
                Some(std::io::ErrorKind::NotFound),
                false,
                -1,
                ""
            ),
            outcome::COMMAND_NOT_FOUND
        );
        assert_eq!(
            classify_outcome(
                "cargo_test",
                Some(std::io::ErrorKind::PermissionDenied),
                false,
                -1,
                ""
            ),
            outcome::SPAWN_FAILED
        );
    }

    #[test]
    fn classify_timeout_beats_exit_code() {
        assert_eq!(
            classify_outcome("cargo_test", None, true, -1, ""),
            outcome::TIMEOUT
        );
    }

    #[test]
    fn classify_zero_exit_is_tests_passed_for_any_runner() {
        for id in ["cargo_test", "python_pytest"] {
            assert_eq!(
                classify_outcome(id, None, false, 0, ""),
                outcome::TESTS_PASSED
            );
        }
        // python_test_file additionally requires the harness OK line:
        // in-process workspace code can os._exit(0) and skip all tests.
        assert_eq!(
            classify_outcome(
                "python_test_file",
                None,
                false,
                0,
                "kernel test harness: OK\n"
            ),
            outcome::TESTS_PASSED
        );
    }

    #[test]
    fn classify_harness_exit0_without_ok_marker_is_not_a_pass() {
        // The os._exit(0) spoof: SUT kills the harness at import time.
        assert_eq!(
            classify_outcome("python_test_file", None, false, 0, ""),
            outcome::TESTS_FAILED
        );
        // Stray output without the marker is also not a pass.
        assert_eq!(
            classify_outcome("python_test_file", None, false, 0, "some print\n"),
            outcome::TESTS_FAILED
        );
    }

    #[test]
    fn os_exit_zero_from_sut_does_not_fabricate_pass() {
        // End-to-end: model-patched SUT kills the harness process with
        // os._exit(0) at import time. Must NOT produce tests_passed.
        let ws = unique_dir("osexit_spoof");
        std::fs::write(ws.join("sut.py"), "import os\nos._exit(0)\n").unwrap();
        std::fs::write(
            ws.join("test_x.py"),
            "from sut import *\n\ndef test_real():\n    assert True\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed, "os._exit(0) spoof must not pass");
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        assert_eq!(report.exit_code, 0, "the spoof really did exit 0");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn classify_cargo_semantics_distinguish_test_failure_from_compile_failure() {
        // exit 101 WITH a "test result:" line => tests actually ran & failed.
        assert_eq!(
            classify_outcome(
                "cargo_test",
                None,
                false,
                101,
                "test result: FAILED. 1 passed;"
            ),
            outcome::TESTS_FAILED
        );
        // exit 101 WITHOUT it => compile/infrastructure failure, NOT a test
        // failure — "exit != 0" must never be assumed to mean tests failed.
        assert_eq!(
            classify_outcome(
                "cargo_test",
                None,
                false,
                101,
                "error[E0308]: mismatched types"
            ),
            outcome::INFRASTRUCTURE_ERROR
        );
    }

    #[test]
    fn classify_pytest_semantics() {
        assert_eq!(
            classify_outcome("python_pytest", None, false, 1, ""),
            outcome::TESTS_FAILED
        );
        for code in [2i32, 3, 4, 5] {
            assert_eq!(
                classify_outcome("python_pytest", None, false, code, ""),
                outcome::INFRASTRUCTURE_ERROR,
                "pytest exit {code} is not a plain test failure"
            );
        }
    }

    #[test]
    fn classify_plain_python_script_failure_is_tests_failed() {
        assert_eq!(
            classify_outcome("python_test_file", None, false, 1, ""),
            outcome::TESTS_FAILED
        );
    }

    #[test]
    fn run_tests_reports_classification_for_failing_tests() {
        let ws = unique_dir("cls_fail");
        std::fs::write(ws.join("test_fail.py"), "assert 1 == 2\n").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn run_tests_reports_timeout_classification() {
        let ws = unique_dir("cls_timeout");
        std::fs::write(ws.join("test_slow.py"), "import time\ntime.sleep(30)\n").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 1).unwrap();
        assert!(!report.passed);
        assert!(report.timed_out);
        assert_eq!(report.classification, outcome::TIMEOUT);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn run_tests_reports_command_not_found_when_runner_missing() {
        // A Cargo.toml workspace derives cargo_test; hide PATH so `cargo`
        // cannot be found by the spawned child lookup.
        let ws = unique_dir("cls_notfound");
        std::fs::write(ws.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        // Derive with a bogus program by pointing PATH at an empty dir is
        // environment-mutating; instead verify the classification function
        // contract directly for this shape.
        assert_eq!(
            classify_outcome(
                "cargo_test",
                Some(std::io::ErrorKind::NotFound),
                false,
                -1,
                ""
            ),
            outcome::COMMAND_NOT_FOUND
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    // ── 4H §5 audit: python-mode matrix ────────────────────────────────

    #[test]
    fn audit_main_guarded_failing_test_fails() {
        let ws = unique_dir("audit_guard_fail");
        std::fs::write(
            ws.join("test_guard.py"),
            "def test_bad():\n    assert 1 == 2\n\nif __name__ == \"__main__\":\n    test_bad()\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed, "main-guarded failing test must fail");
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn audit_main_guarded_passing_test_passes() {
        let ws = unique_dir("audit_guard_pass");
        std::fs::write(
            ws.join("test_guard.py"),
            "def test_ok():\n    assert 1 == 1\n\nif __name__ == \"__main__\":\n    test_ok()\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(report.passed);
        assert_eq!(report.exit_code, 0);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn audit_non_assert_exception_in_test_fails() {
        let ws = unique_dir("audit_exc");
        std::fs::write(
            ws.join("test_exc.py"),
            "def test_boom():\n    raise ValueError('kaboom')\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        assert!(
            report.stderr_tail.contains("ValueError"),
            "stderr_tail: {}",
            report.stderr_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn audit_sys_exit_nonzero_fails_with_code() {
        let ws = unique_dir("audit_exit");
        std::fs::write(ws.join("test_exit.py"), "import sys\nsys.exit(3)\n").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert_eq!(report.exit_code, 3);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn audit_import_failure_fails() {
        let ws = unique_dir("audit_import");
        std::fs::write(
            ws.join("test_badimport.py"),
            "from nonexistent_module_xyz import thing\n\ndef test_x():\n    assert True\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        assert!(
            report.stderr_tail.contains("ModuleNotFoundError"),
            "stderr_tail: {}",
            report.stderr_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn audit_empty_test_file_documents_current_semantics() {
        // An empty test file executes nothing and exits 0. Current
        // semantics: PASSED (the "script is the test" contract — nothing
        // failed). Registered as OBSERVATION in 4H_DEFECT_REGISTER: a
        // no-test file masquerading as tests is spiritually vacuous, but
        // failing it would break legitimate import-time-assert scripts.
        let ws = unique_dir("audit_empty");
        std::fs::write(ws.join("test_empty.py"), "").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(report.passed, "documented current semantics");
        assert_eq!(report.exit_code, 0);
        let _ = std::fs::remove_dir_all(&ws);
    }

    // ── C0: failing-test identity for the feedback loop ──────────────

    #[test]
    fn failures_carry_names_of_failed_tests_only() {
        let ws = unique_dir("c0_names");
        std::fs::write(
            ws.join("test_x.py"),
            "def test_ok():\n    assert True\n\ndef test_bad():\n    assert False\n\ndef test_also_bad():\n    assert 1 == 2\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert_eq!(report.classification, outcome::TESTS_FAILED);
        assert_eq!(report.failures, vec!["test_also_bad", "test_bad"]);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn failures_empty_on_pass_and_on_module_body_failure() {
        let ws = unique_dir("c0_pass");
        std::fs::write(ws.join("test_ok.py"), "def test_ok():\n    assert True\n").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(report.passed);
        assert!(report.failures.is_empty());
        let _ = std::fs::remove_dir_all(&ws);

        // Module-body failure: no test ran, so there is no located rung.
        let ws = unique_dir("c0_bodyfail");
        std::fs::write(
            ws.join("test_badimport.py"),
            "from nonexistent_module_xyz import thing\n\ndef test_x():\n    assert True\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert!(report.failures.is_empty(), "no test ran: no names");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn fake_marker_from_test_file_cannot_shadow_kernel_marker() {
        // The test file is workspace content and must not be able to
        // misdirect the feedback loop by printing a forged marker. The
        // harness prints its marker last; the kernel parses the last one.
        let ws = unique_dir("c0_inject");
        std::fs::write(
            ws.join("test_x.py"),
            "import sys\nprint('DAK_TEST_FAILURES_V1 [\"forged_name\"]', file=sys.stderr)\n\ndef test_real_failure():\n    assert False\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert_eq!(report.failures, vec!["test_real_failure"]);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn parse_failures_fail_closed_on_malformed_marker() {
        assert!(parse_failures("DAK_TEST_FAILURES_V1 {not json}\n").is_empty());
        assert!(parse_failures("DAK_TEST_FAILURES_V1 [\"ok\", 42]\n").is_empty());
        assert!(parse_failures("no marker here\n").is_empty());
        // Non-identifier names are filtered out.
        assert_eq!(
            parse_failures("DAK_TEST_FAILURES_V1 [\"test_ok\", \"bad name\", \"\"]\n"),
            vec!["test_ok"]
        );
    }

    #[test]
    fn report_without_failures_field_still_deserializes() {
        // Reports persisted before C0 have no `failures` key.
        let legacy = serde_json::json!({
            "version": "test_report_v1",
            "command_id": "python_test_file",
            "argv": ["python3"],
            "exit_code": 0,
            "passed": true,
            "timed_out": false,
            "classification": "tests_passed",
            "stdout_tail": "",
            "stderr_tail": "",
            "duration_ms": 1,
            "workspace": "/tmp/ws",
            "captured_unix": 1
        });
        let report: TestReportV1 = serde_json::from_value(legacy).unwrap();
        assert!(report.failures.is_empty());
    }

    #[test]
    fn stale_pycache_does_not_shadow_repatched_source() {
        // The feedback loop re-patches and re-runs within the same second;
        // Python's timestamp+size pyc validation then treats the OLD
        // bytecode as valid. The harness purges __pycache__ before running.
        let ws = unique_dir("stale_pyc");
        std::fs::write(ws.join("calc.py"), "def value():\n    return 1\n").unwrap();
        std::fs::write(
            ws.join("test_x.py"),
            "from calc import value\n\ndef test_value():\n    assert value() == 1\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(report.passed);

        // Re-patch with same-length different content and a matching test.
        std::fs::write(ws.join("calc.py"), "def value():\n    return 2\n").unwrap();
        std::fs::write(
            ws.join("test_x.py"),
            "from calc import value\n\ndef test_value():\n    assert value() == 2\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(
            report.passed,
            "stale bytecode must not shadow the re-patched source: {}",
            report.stderr_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    // ── M-2: Seatbelt sandbox around the python_test_file harness ──────
    // (macOS only; other platforms pass through unsandboxed)
    // These tests share the process-global DAK_TEST_SANDBOX env — serialize them.
    #[cfg(target_os = "macos")]
    static SANDBOX_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_wraps_command_and_preserves_pass_fail() {
        let _g = SANDBOX_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ws = unique_dir("m2_wrap");
        std::fs::write(ws.join("test_x.py"), "def test_ok():\n    assert True\n").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(report.passed);
        assert_eq!(
            report.argv.first().map(String::as_str),
            Some("/usr/bin/sandbox-exec"),
            "evidence argv must show the sandbox wrapper"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_denies_network() {
        let _g = SANDBOX_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ws = unique_dir("m2_net");
        std::fs::write(
            ws.join("test_x.py"),
            "import socket\n\ndef test_net():\n    socket.create_connection(('127.0.0.1', 9), timeout=2)\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed);
        assert!(
            report.stderr_tail.contains("PermissionError")
                || report.stderr_tail.contains("Operation not permitted"),
            "expected sandbox EPERM, got: {}",
            report.stderr_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_denies_write_outside_workspace() {
        let _g = SANDBOX_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ws = unique_dir("m2_write");
        let escape = std::env::temp_dir().join(format!(
            "m2_escape_probe_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let escape_str = escape.to_string_lossy().into_owned();
        let test = format!("def test_escape():\n    open({escape_str:?}, 'w').write('x')\n");
        std::fs::write(ws.join("test_x.py"), test).unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed, "write outside workspace must fail");
        assert!(!escape.exists(), "escape file must not be created");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_denies_credential_read() {
        let _g = SANDBOX_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ws = unique_dir("m2_read");
        std::fs::write(
            ws.join("test_x.py"),
            "import os\n\ndef test_creds():\n    os.listdir(os.path.expanduser('~/.ssh'))\n",
        )
        .unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert!(!report.passed, "~/.ssh read must be denied");
        assert!(
            report.stderr_tail.contains("PermissionError")
                || report.stderr_tail.contains("Operation not permitted"),
            "expected sandbox EPERM, got: {}",
            report.stderr_tail
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_kill_switch() {
        let _g = SANDBOX_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("DAK_TEST_SANDBOX", "off");
        let ws = unique_dir("m2_off");
        std::fs::write(ws.join("test_x.py"), "def test_ok():\n    assert True\n").unwrap();
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        std::env::remove_var("DAK_TEST_SANDBOX");
        assert!(report.passed);
        assert_eq!(
            report.argv.first().map(String::as_str),
            Some("python3"),
            "kill switch must bypass the wrapper"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sandbox_backend_is_recorded_honestly() {
        let _g = SANDBOX_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ws = unique_dir("m2_backend");
        std::fs::write(ws.join("test_x.py"), "def test_ok():\n    assert True\n").unwrap();

        // sandboxed run reports seatbelt
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        assert_eq!(report.sandbox_backend, "seatbelt");

        // kill switch reports none — never overclaimed
        std::env::set_var("DAK_TEST_SANDBOX", "off");
        let report = run_tests(ws.to_str().unwrap(), 30).unwrap();
        std::env::remove_var("DAK_TEST_SANDBOX");
        assert_eq!(report.sandbox_backend, "none");

        // reports persisted before this field still deserialize as "none"
        let mut legacy = serde_json::to_value(&report).unwrap();
        legacy.as_object_mut().unwrap().remove("sandbox_backend");
        let report: TestReportV1 = serde_json::from_value(legacy).unwrap();
        assert_eq!(report.sandbox_backend, "none");

        let _ = std::fs::remove_dir_all(&ws);
    }
}
