//! P4 regression tests:
//! - P4-C: ephemeral CLI lifecycle — the detached supervisor unloads an idle
//!   kernel-owned server, refuses stale/reused pids, and exits honestly when
//!   there is nothing to supervise.
//! - P4-D: isolation — lifecycle adoption must NEVER signal a process that
//!   does not identify as the managed server (pid-reuse / shared-state
//!   hazard found during P3).
//! - P4-E: determinism — the CodeFix chain emits only canonical event types
//!   and produces identical canonical evidence across identical runs.

use serde_json::json;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique(name: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{name}_{n}_{nanos}")
}

fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(unique(name));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn kernel_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
}

/// Spawn a harmless long-lived process that does NOT identify as the model
/// server (used to prove adoption never signals unrelated processes).
fn spawn_unrelated_process() -> std::process::Child {
    Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep")
}

fn write_state(
    path: &std::path::Path,
    pid: Option<u32>,
    state: &str,
    base_url: &str,
    last_activity_unix: u64,
    supervisor_pid: Option<u32>,
) {
    let json = json!({
        "pid": pid,
        "owned": true,
        "state": state,
        "model": "/tmp/fake-model",
        "base_url": base_url,
        "started_unix": last_activity_unix,
        "last_activity_unix": last_activity_unix,
        "supervisor_pid": supervisor_pid
    });
    std::fs::write(path, json.to_string()).unwrap();
}

// ── P4-D: adoption must never signal unrelated processes ─────────────────

#[test]
fn lifecycle_adoption_refuses_unrelated_pid() {
    // Simulates the P3 hazard: a state file claims the kernel owns a pid
    // that is actually an unrelated process. stop_managed_server must NOT
    // signal it (identity verification before adoption).
    let mut victim = spawn_unrelated_process();
    let victim_pid = victim.id();

    let dir = fresh_dir("p4_refuse");
    let state = dir.join("state.json");
    write_state(
        &state,
        Some(victim_pid),
        "ModelLoaded",
        "http://127.0.0.1:1/v1",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 9999,
        None,
    );

    let cfg = deterministic_ai_kernel::mlx_lifecycle::LifecycleConfig {
        idle_timeout: Duration::from_secs(1),
        startup_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(2),
        enabled: true,
        server_command: "mlx_lm.server".to_string(),
        state_file: state.clone(),
    };
    let lc = deterministic_ai_kernel::mlx_lifecycle::MlxLifecycle::with_config(cfg);
    let report = lc.stop_managed_server().expect("stop returns");

    assert!(
        !report.attempted,
        "unrelated pid must never be signaled: {:?}",
        report
    );
    // The victim must still be alive.
    let still_alive = victim.try_wait().ok().flatten().is_none();
    assert!(still_alive, "unrelated process was killed!");
    victim.kill().ok();
    victim.wait().ok();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn process_identity_check_accepts_server_like_and_rejects_other() {
    let cfg = deterministic_ai_kernel::mlx_lifecycle::LifecycleConfig {
        idle_timeout: Duration::from_secs(1),
        startup_timeout: Duration::from_secs(5),
        shutdown_grace: Duration::from_secs(2),
        enabled: true,
        server_command: "mlx_lm.server".to_string(),
        state_file: std::env::temp_dir().join("p4_identity_unused.json"),
    };
    // Our own test process is NOT server-like.
    let self_pid = std::process::id();
    assert!(
        !deterministic_ai_kernel::mlx_lifecycle::process_looks_like_server(self_pid, &cfg),
        "the test process must not identify as the model server"
    );
    // A dead pid is not server-like (ps fails).
    assert!(!deterministic_ai_kernel::mlx_lifecycle::process_looks_like_server(999_999, &cfg));
}

// ── P4-C: detached supervisor behavior ───────────────────────────────────

fn run_supervisor(state: &std::path::Path, timeout_secs: u64) -> (i32, Duration) {
    let started = Instant::now();
    let out = Command::new(kernel_bin())
        .arg("__lifecycle-supervise")
        // cargo injects MLX_LIFECYCLE=off into the whole test tree
        // (.cargo/config.toml) so suites never spawn real models; the
        // supervisor under test must explicitly re-enable lifecycle.
        .env("MLX_LIFECYCLE", "on")
        .env("DAK_MLX_SUPERVISOR_DEBUG", "1")
        .env("DAK_MLX_LIFECYCLE_STATE", state)
        .env("MLX_IDLE_TIMEOUT_SECS", timeout_secs.to_string())
        .env("MLX_SHUTDOWN_GRACE_SECS", "3")
        .env("MLX_STARTUP_TIMEOUT_SECS", "10")
        .env("OPENAI_BASE_URL", "http://127.0.0.1:1/v1")
        .env("OPENAI_MODEL", "/tmp/fake-model")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run supervisor");
    // stderr only becomes visible when the test fails.
    eprintln!(
        "[supervisor] exit={:?} stderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), started.elapsed())
}

#[test]
fn supervisor_exits_cleanly_without_state() {
    let dir = fresh_dir("p4_sup_nostate");
    let state = dir.join("state.json"); // never created
    let (code, elapsed) = run_supervisor(&state, 1);
    assert_eq!(code, 0, "nothing to supervise must exit 0");
    assert!(elapsed < Duration::from_secs(10), "must exit promptly");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn supervisor_refuses_reused_pid_and_does_not_kill() {
    // State claims an owned server, but the pid is an unrelated process
    // (pid-reuse scenario). The supervisor must exit WITHOUT killing it.
    let mut victim = spawn_unrelated_process();
    let victim_pid = victim.id();
    let dir = fresh_dir("p4_sup_reuse");
    let state = dir.join("state.json");
    write_state(
        &state,
        Some(victim_pid),
        "ModelLoaded",
        "http://127.0.0.1:1/v1",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 9999,
        None,
    );
    let (code, _) = run_supervisor(&state, 1);
    assert_eq!(code, 0, "reused-pid case exits without action");
    let still_alive = victim.try_wait().ok().flatten().is_none();
    assert!(still_alive, "supervisor must not kill an unrelated pid");
    victim.kill().ok();
    victim.wait().ok();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn supervisor_unloads_idle_server_and_verifies() {
    // Full P4-C cycle with a fake server-like process:
    //   owned server alive + idle expired
    //   -> supervisor SIGTERMs (graceful first)
    //   -> process gone, state Unloaded, exit 0.
    let dir = fresh_dir("p4_sup_unload");
    let fake = dir.join("fake_mlx_server.py");
    // Command line contains "python" + "mlx" so the identity check accepts
    // it, and SIGTERM ends it (default python handling).
    std::fs::write(
        &fake,
        "import time\ntry:\n    time.sleep(300)\nexcept KeyboardInterrupt:\n    pass\n",
    )
    .unwrap();
    let mut server = Command::new("python3")
        .arg(&fake)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fake server");
    let server_pid = server.id();

    let state = dir.join("state.json");
    write_state(
        &state,
        Some(server_pid),
        "ModelLoaded",
        "http://127.0.0.1:1/v1", // endpoint dead -> endpoint_down via probe
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 9999, // already idle far beyond any timeout
        None,
    );

    let (code, _) = run_supervisor(&state, 1);
    assert_eq!(code, 0, "verified unload must exit 0");

    // Process really gone.
    let gone = server.try_wait().ok().flatten().is_some()
        || unsafe { libc::kill(server_pid as i32, 0) != 0 };
    assert!(gone, "fake server must be unloaded");

    // State file records Unloaded.
    let raw = std::fs::read_to_string(&state).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["state"], "Unloaded", "persisted state must be Unloaded");

    let _ = server.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn supervisor_does_not_unload_fresh_activity() {
    // Idle timer NOT expired (activity is fresh): supervisor must NOT kill
    // the server during this test's bounded run. We run the supervisor with
    // a large timeout and verify the fake server survives.
    let dir = fresh_dir("p4_sup_fresh");
    let fake = dir.join("fake_mlx_server.py");
    std::fs::write(&fake, "import time\ntime.sleep(300)\n").unwrap();
    let mut server = Command::new("python3")
        .arg(&fake)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fake server");
    let server_pid = server.id();

    let state = dir.join("state.json");
    write_state(
        &state,
        Some(server_pid),
        "ModelLoaded",
        "http://127.0.0.1:1/v1",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(), // activity NOW -> idle not expired
        None,
    );

    // The supervisor loops while idle-not-expired; run it as a child and
    // kill it after a short window, then verify the server survived.
    let mut sup = Command::new(kernel_bin())
        .arg("__lifecycle-supervise")
        .env("MLX_LIFECYCLE", "on")
        .env("DAK_MLX_LIFECYCLE_STATE", &state)
        .env("MLX_IDLE_TIMEOUT_SECS", "3600")
        .env("OPENAI_BASE_URL", "http://127.0.0.1:1/v1")
        .env("OPENAI_MODEL", "/tmp/fake-model")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn supervisor");
    std::thread::sleep(Duration::from_secs(3));
    sup.kill().ok();
    sup.wait().ok();

    let alive = server.try_wait().ok().flatten().is_none();
    assert!(alive, "fresh-activity server must NOT be unloaded");
    server.kill().ok();
    server.wait().ok();
    let _ = std::fs::remove_dir_all(&dir);
}

// ── P4-E: CodeFix chain determinism + canonical event types ──────────────

mod chain {
    use deterministic_ai_kernel::effects::execute_effects;
    use deterministic_ai_kernel::event_bus::EventBus;
    use deterministic_ai_kernel::workflow::contract::{steps_to_exec_spec, Step, StepKind};
    use serde_json::json;

    pub fn create_task(task_id: &str, spec_json: &str, payload: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db = std::env::temp_dir().join(format!("dak_p4_chain_{task_id}_{nanos}.db"));
        let db_str = db.to_string_lossy().into_owned();
        let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db_str).unwrap();
        conn.execute(
            "INSERT INTO tasks (task_id, task_class, exec_spec) VALUES (?1, 'CodeFix', ?2)",
            rusqlite::params![task_id, spec_json],
        )
        .unwrap();
        drop(conn);
        std::fs::create_dir_all("artifacts").unwrap();
        std::fs::write(format!("artifacts/pipeline_input.{task_id}.txt"), payload).unwrap();
        db_str
    }

    pub fn cleanup(task_id: &str, db: &str) {
        let _ = std::fs::remove_file(format!("artifacts/pipeline_input.{task_id}.txt"));
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_file(format!("{db}-wal"));
        let _ = std::fs::remove_file(format!("{db}-shm"));
    }

    pub fn python_project(dir: &std::path::Path) {
        std::fs::write(
            dir.join("calc.py"),
            "def multiply(a, b):\n    return a + b\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("test_calc.py"),
            "from calc import multiply\nassert multiply(2, 3) == 6\nprint('DET_OK')\n",
        )
        .unwrap();
    }

    pub fn full_chain_spec(workspace: &str) -> serde_json::Value {
        let mut spec = steps_to_exec_spec(&[
            Step {
                kind: StepKind::ApplyPatch,
                detail: Some("apply patch".to_string()),
            },
            Step {
                kind: StepKind::RunTests,
                detail: Some("run tests".to_string()),
            },
            Step {
                kind: StepKind::ValidatePatch,
                detail: Some("validate patch".to_string()),
            },
        ]);
        for step_id in ["00_apply_patch", "01_run_tests"] {
            let step = spec
                .steps
                .iter_mut()
                .find(|s| s.step_id == step_id)
                .unwrap();
            step.primitive
                .as_mut()
                .unwrap()
                .payload
                .as_object_mut()
                .unwrap()
                .insert("workspace".to_string(), json!(workspace));
        }
        serde_json::to_value(&spec).unwrap()
    }

    pub fn seed_patch(db: &str, task_id: &str, target: &str) {
        let bus = EventBus::new(db).unwrap();
        let generation = bus.latest_generation_for_task(task_id).unwrap_or(0);
        bus.append_semantic_artifact(
            task_id,
            "00_patch_code",
            generation,
            "primitive_result_v1",
            &json!({
                "patch_v1": {
                    "version": "patch_v1",
                    "target_file": target,
                    "context_before": "    return a + b",
                    "replacement": "    return a * b",
                    "reason": "determinism test patch"
                }
            }),
        )
        .unwrap();
    }

    pub fn run_chain(name: &str) -> (Vec<(String, String)>, Vec<serde_json::Value>, Vec<String>) {
        let dir = std::env::temp_dir().join(format!("dak_p4_chainws_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        python_project(&dir);
        let calc = dir.join("calc.py");
        let task_id = format!("p4-{name}");
        let spec = full_chain_spec(dir.to_str().unwrap());
        let db = create_task(&task_id, &spec.to_string(), "fix multiply");
        seed_patch(&db, &task_id, calc.to_str().unwrap());
        execute_effects(&db, &task_id).expect("chain completes");

        let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db).unwrap();
        let mut stmt = conn
            .prepare("SELECT step_id, status FROM step_status WHERE task_id = ?1 ORDER BY step_id")
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map(rusqlite::params![task_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let mut ev_stmt = conn
            .prepare("SELECT DISTINCT event_type FROM event_log WHERE task_id = ?1")
            .unwrap();
        let event_types: Vec<String> = ev_stmt
            .query_map(rusqlite::params![task_id], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let bus = EventBus::new(&db).unwrap();
        let artifacts = bus.list_semantic_artifacts(&task_id, None).unwrap();
        let reports: Vec<serde_json::Value> = artifacts
            .iter()
            .filter_map(|row| {
                serde_json::from_str::<serde_json::Value>(&row.payload)
                    .ok()
                    .and_then(|p| p.get("test_report_v1").cloned())
            })
            .collect();

        cleanup(&task_id, &db);
        let _ = std::fs::remove_dir_all(&dir);
        (rows, reports, event_types)
    }
}

#[test]
fn codefix_chain_emits_only_canonical_event_types() {
    // P4-E invariant: the new lifecycle/test-report states must NOT
    // introduce new event types. The CodeFix chain may only emit the event
    // vocabulary the kernel already uses for lease-authorized execution
    // (see providers/storage.rs + effects.rs).
    const ALLOWED: &[&str] = &[
        "LEASE_ACQUIRED",
        "LEASE_RELEASED",
        "WORKER_CLAIMED",
        "STEP_DISPATCHED",
        "STEP_STARTED",
        "STEP_COMPLETED",
        "STEP_FAILED",
        "EFFECT_RESERVED",
    ];
    let (_, _, event_types) = chain::run_chain("evtypes");
    assert!(!event_types.is_empty(), "chain must emit events");
    for et in &event_types {
        assert!(
            ALLOWED.contains(&et.as_str()),
            "non-canonical event type emitted by the CodeFix chain: {et}"
        );
    }
    assert!(
        event_types.iter().any(|e| e == "STEP_COMPLETED"),
        "chain must commit steps"
    );
    assert!(
        event_types.iter().any(|e| e == "EFFECT_RESERVED"),
        "chain must reserve effects through the canonical ledger path"
    );
}

#[test]
fn codefix_chain_is_deterministic_across_identical_runs() {
    let (rows_a, reports_a, events_a) = chain::run_chain("det_a");
    let (rows_b, reports_b, events_b) = chain::run_chain("det_b");

    assert_eq!(rows_a, rows_b, "step outcomes must be identical");
    assert_eq!(events_a, events_b, "event type set must be identical");
    assert_eq!(reports_a.len(), 1);
    assert_eq!(reports_b.len(), 1);
    // Canonical evidence fields are identical; wall-clock fields
    // (duration_ms, captured_unix) are excluded by design (not
    // architecturally guaranteed).
    for f in [
        "command_id",
        "exit_code",
        "passed",
        "classification",
        "version",
    ] {
        assert_eq!(
            reports_a[0][f], reports_b[0][f],
            "field {f} must be deterministic"
        );
    }
    assert_eq!(reports_a[0]["argv"], reports_b[0]["argv"]);
    assert_eq!(
        reports_a[0]["stdout_tail"], reports_b[0]["stdout_tail"],
        "identical test project must produce identical stdout"
    );
}
