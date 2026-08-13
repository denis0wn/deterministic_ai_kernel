//! P0 MLX lifecycle integration tests — real process lifecycle with a
//! lightweight fake server (NOT the 6.3 GB model).
//!
//! The fake server is a real OS process speaking the mlx_lm.server health
//! contract (GET /v1/models -> OpenAI-style JSON) and exiting on SIGTERM.
//! This proves the real spawn/readiness/SIGTERM/verify path end-to-end.

use deterministic_ai_kernel::mlx_lifecycle::{
    self as mlx, LifecycleConfig, LifecycleState, MlxLifecycle,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique(name: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{name}_{n}_{nanos}")
}

/// Write the fake mlx server (wrapper + python HTTP responder) and return
/// the wrapper path. The wrapper accepts the same CLI shape as
/// mlx_lm.server (--model … --port N …) and serves /v1/models.
fn write_fake_server() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(unique("dak_mlx_fake"));
    std::fs::create_dir_all(&dir).unwrap();

    let py = dir.join("fake_mlx.py");
    std::fs::write(
        &py,
        r#"
import http.server, json, signal, sys

class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith('/v1/models') or self.path == '/models':
            body = json.dumps({"object": "list", "data": [{"id": "fake-model"}]}).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()
    def log_message(self, *args):
        pass

def _term(*_):
    sys.exit(0)

signal.signal(signal.SIGTERM, _term)
http.server.HTTPServer(('127.0.0.1', int(sys.argv[1])), Handler).serve_forever()
"#,
    )
    .unwrap();

    let wrapper = dir.join("fake_mlx_server");
    std::fs::write(
        &wrapper,
        format!(
            r#"#!/bin/sh
PORT=""
while [ $# -gt 0 ]; do
  case "$1" in
    --port) PORT="$2"; shift 2 ;;
    *) shift ;;
  esac
done
exec python3 "{}" "$PORT"
"#,
            py.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (wrapper, dir)
}

fn pick_port() -> u16 {
    // Ask the OS for a free port, then release it.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn manager(
    wrapper: &std::path::Path,
    port: u16,
    idle_secs: u64,
) -> (MlxLifecycle, String, PathBuf) {
    let state_file = std::env::temp_dir().join(format!("{}.json", unique("dak_mlx_state")));
    let cfg = LifecycleConfig {
        idle_timeout: Duration::from_secs(idle_secs),
        startup_timeout: Duration::from_secs(30),
        shutdown_grace: Duration::from_secs(10),
        enabled: true,
        server_command: wrapper.to_string_lossy().into_owned(),
        state_file: state_file.clone(),
    };
    let base = format!("http://127.0.0.1:{port}/v1");
    (MlxLifecycle::with_config(cfg), base, state_file)
}

#[test]
fn full_cycle_start_infer_activity_idle_unload_reload() {
    let (wrapper, dir) = write_fake_server();
    let port = pick_port();
    let (lc, base, state_file) = manager(&wrapper, port, 1);
    std::env::set_var("OPENAI_MODEL", "/tmp/fake-model");

    // MODEL_START -> MODEL_LOADED
    lc.ensure_ready(&base).expect("startup must succeed");
    assert_eq!(lc.status().state, LifecycleState::ModelLoaded);
    let pid = lc.status().pid.expect("owned pid");
    assert!(mlx::probe_endpoint(&base), "endpoint must be up");

    // Request resets the idle timer: activity now, shutdown not due yet.
    lc.record_activity();
    assert!(!lc.idle_shutdown_due(), "fresh activity resets the timer");

    // IDLE_TIMEOUT: with a 1 s timeout, expiry makes shutdown due. Sleep a
    // little longer than the timeout, then allow up to 5 s for any
    // concurrently-running test in this binary to release the global
    // in-flight inference guard (idle_shutdown_due requires in_flight == 0).
    std::thread::sleep(Duration::from_millis(1100));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !lc.idle_shutdown_due() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        lc.idle_shutdown_due(),
        "idle shutdown must be due after timeout from last activity"
    );

    // GRACEFUL_SHUTDOWN -> MODEL_UNLOADED (verified)
    let report = lc.stop_managed_server().expect("graceful stop");
    assert!(report.attempted);
    assert!(
        !report.escalated_to_kill,
        "SIGTERM must suffice for a healthy server"
    );
    assert!(report.process_gone, "process must be gone");
    assert!(report.endpoint_down, "endpoint must be closed");
    assert!(report.verified());
    assert_eq!(lc.status().state, LifecycleState::Unloaded);

    // Multiple independent observations of unload.
    assert!(!mlx::probe_endpoint(&base), "endpoint down after unload");
    #[cfg(unix)]
    {
        let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
        assert!(!alive, "pid must not exist after unload");
    }

    // RELOAD: next request starts the server again and reaches loaded state.
    lc.ensure_ready(&base).expect("reload must succeed");
    assert_eq!(lc.status().state, LifecycleState::ModelLoaded);
    let pid2 = lc.status().pid.expect("owned pid after reload");
    assert_ne!(pid2, pid, "reload spawns a new process");
    assert!(mlx::probe_endpoint(&base));

    // Cleanup.
    lc.stop_managed_server().expect("final stop");
    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_requests_serialize_and_share_one_server() {
    let (wrapper, dir) = write_fake_server();
    let port = pick_port();
    let (lc, base, state_file) = manager(&wrapper, port, 60);
    std::env::set_var("OPENAI_MODEL", "/tmp/fake-model");

    let lc = std::sync::Arc::new(lc);
    let mut handles = vec![];
    for _ in 0..8 {
        let lc = lc.clone();
        let base = base.clone();
        handles.push(std::thread::spawn(move || {
            lc.ensure_ready(&base).expect("concurrent ensure_ready");
            lc.record_activity();
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    let st = lc.status();
    assert_eq!(st.state, LifecycleState::ModelLoaded);
    assert!(
        st.pid.is_some(),
        "exactly one owned server for all requests"
    );
    assert!(mlx::probe_endpoint(&base));

    lc.stop_managed_server().expect("stop");
    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn in_flight_inference_blocks_idle_shutdown_decision() {
    let (wrapper, dir) = write_fake_server();
    let port = pick_port();
    let (lc, base, state_file) = manager(&wrapper, port, 1);
    std::env::set_var("OPENAI_MODEL", "/tmp/fake-model");

    lc.ensure_ready(&base).expect("startup");
    let guard = mlx::begin_inference(); // INFERENCE in progress
    std::thread::sleep(Duration::from_millis(1100));
    assert!(
        !lc.idle_shutdown_due(),
        "watcher must never stop a server with in-flight inference"
    );
    drop(guard);
    assert!(
        lc.idle_shutdown_due(),
        "after inference ends and timeout passed, shutdown is due"
    );

    lc.stop_managed_server().expect("stop");
    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn timeout_zero_disables_auto_unload() {
    let (wrapper, dir) = write_fake_server();
    let port = pick_port();
    let (lc, base, state_file) = manager(&wrapper, port, 0);
    std::env::set_var("OPENAI_MODEL", "/tmp/fake-model");

    lc.ensure_ready(&base).expect("startup");
    std::thread::sleep(Duration::from_millis(1100));
    assert!(
        !lc.idle_shutdown_due(),
        "timeout=0 must disable automatic unload"
    );

    lc.stop_managed_server().expect("explicit stop still works");
    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn configured_timeout_is_the_one_enforced() {
    let (wrapper, dir) = write_fake_server();
    let port = pick_port();
    let (lc, base, state_file) = manager(&wrapper, port, 60);
    std::env::set_var("OPENAI_MODEL", "/tmp/fake-model");

    lc.ensure_ready(&base).expect("startup");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !lc.idle_shutdown_due(),
        "with a 60 s configured timeout, 1.5 s idle must NOT trigger shutdown"
    );

    lc.stop_managed_server().expect("stop");
    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stale_owned_state_with_dead_pid_is_cleaned_and_recovers() {
    let (wrapper, dir) = write_fake_server();
    let port = pick_port();
    let (lc, base, state_file) = manager(&wrapper, port, 60);
    std::env::set_var("OPENAI_MODEL", "/tmp/fake-model");

    // Orphan scenario: state file claims an owned server that is dead.
    let stale = serde_json::json!({
        "pid": 999_999u32,
        "owned": true,
        "state": "ModelLoaded",
        "model": "/tmp/fake-model",
        "base_url": base,
        "started_unix": 0u64,
        "last_activity_unix": 0u64
    });
    std::fs::write(&state_file, stale.to_string()).unwrap();

    // ensure_ready must detect the orphan, clean it, and start fresh.
    lc.ensure_ready(&base).expect("recovery after orphan state");
    assert_eq!(lc.status().state, LifecycleState::ModelLoaded);
    assert!(mlx::probe_endpoint(&base));
    let pid = lc.status().pid.expect("new owned pid");
    assert_ne!(pid, 999_999);

    lc.stop_managed_server().expect("stop");
    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn startup_failure_when_server_exits_immediately() {
    // A "server" that exits at once must yield a truthful startup failure,
    // never a ModelLoaded state.
    let dir = std::env::temp_dir().join(unique("dak_mlx_fail"));
    std::fs::create_dir_all(&dir).unwrap();
    let wrapper = dir.join("instant_exit");
    std::fs::write(&wrapper, "#!/bin/sh\nexit 3\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let port = pick_port();
    let (lc, _base, state_file) = manager(&wrapper, port, 60);
    std::env::set_var("OPENAI_MODEL", "/tmp/fake-model");

    let err = lc
        .ensure_ready(&format!("http://127.0.0.1:{port}/v1"))
        .expect_err("server exits during startup");
    assert!(
        err.to_string().contains("exited during startup"),
        "got: {err}"
    );
    assert_eq!(lc.status().state, LifecycleState::Idle);
    assert!(lc.status().last_failure.is_some());
    assert!(lc.status().pid.is_none());

    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shutdown_report_shape_for_unowned_manager() {
    let dir = std::env::temp_dir().join(unique("dak_mlx_unowned"));
    std::fs::create_dir_all(&dir).unwrap();
    let wrapper = dir.join("unused");
    std::fs::write(&wrapper, "#!/bin/sh\nsleep 1\n").unwrap();
    let (lc, _base, state_file) = manager(&wrapper, 1, 60);
    let report = lc.stop_managed_server().expect("noop stop");
    assert!(!report.attempted);
    let _ = std::fs::remove_file(&state_file);
    let _ = std::fs::remove_dir_all(&dir);
}
