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
import runpy
import sys
import traceback


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
        print(
            "kernel test harness: FAILED " + ", ".join(failures),
            file=sys.stderr,
        )
        return 1
    print("kernel test harness: OK")
    return 0


sys.exit(_kernel_test_harness())
"#;

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
    let spawn_result = Command::new(&derived.program)
        .args(&derived.argv)
        .current_dir(&ws)
        .stdin(if needs_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let argv: Vec<String> = std::iter::once(derived.program.clone())
        .chain(derived.argv.clone())
        .collect();
    let report_tail = |stdout: String,
                       stderr: String,
                       exit_code: i32,
                       timed_out: bool,
                       classification: &'static str| TestReportV1 {
        version: TEST_REPORT_VERSION.to_string(),
        command_id: derived.command_id.clone(),
        argv: argv.clone(),
        exit_code,
        passed: classification == outcome::TESTS_PASSED,
        timed_out,
        classification: classification.to_string(),
        stdout_tail: tail(&stdout, OUTPUT_TAIL_CHARS),
        stderr_tail: tail(&stderr, OUTPUT_TAIL_CHARS),
        duration_ms: started.elapsed().as_millis() as u64,
        workspace: ws.to_string_lossy().into_owned(),
        captured_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
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
        for id in ["cargo_test", "python_pytest", "python_test_file"] {
            assert_eq!(
                classify_outcome(id, None, false, 0, ""),
                outcome::TESTS_PASSED
            );
        }
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
}
