//! R4 (HD-3) — stall diagnostics integration test.
//!
//! Artificial stall injection: a local TCP endpoint accepts the connection
//! and then NEVER responds (exactly the B6/C12 failure shape observed in
//! the 2026-08-14 acceptance: endpoint up, zero tokens, 120s silence).
//! Requirements proven here:
//! 1. the request fails with an EXPLICIT timeout attribution
//!    ("TIMED OUT after Ns"), not the opaque "error sending request";
//! 2. a kernel-owned STALL_DETECTED event reaches the event store with
//!    task/step/elapsed_secs/llm_calls/last_known_state;
//! 3. the failure surfaces fast (the timeout window, not 3x retried
//!    silence) and the task never completes.

use deterministic_ai_kernel::effects::execute_effects;
use deterministic_ai_kernel::providers::storage::StorageProvider;
use deterministic_ai_kernel::workflow::contract::{steps_to_exec_spec, Step, StepKind};
use std::io::Read;
use std::net::TcpListener;
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

/// Endpoint that accepts connections and never answers — the stall oracle.
fn hanging_endpoint() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stall oracle");
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        // Accept and hold connections open without producing a byte.
        for stream in listener.incoming() {
            if let Ok(mut s) = stream {
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    // Read the request, then sit on the socket silently.
                    let _ = s.read(&mut buf);
                    std::thread::sleep(Duration::from_secs(120));
                });
            }
        }
    });
    (format!("http://{addr}/v1"), handle)
}

fn create_question_task(task_id: &str, payload: &str) -> String {
    let db = std::env::temp_dir().join(format!("{}.db", unique("dak_r4")));
    let db_str = db.to_string_lossy().into_owned();
    let spec = steps_to_exec_spec(&[Step {
        kind: StepKind::AnswerQuestion,
        detail: Some(payload.to_string()),
    }]);
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db_str).unwrap();
    conn.execute(
        "INSERT INTO tasks (task_id, task_class, exec_spec) VALUES (?1, 'Question', ?2)",
        rusqlite::params![task_id, serde_json::to_string(&spec).unwrap()],
    )
    .unwrap();
    drop(conn);
    std::fs::create_dir_all("artifacts").unwrap();
    std::fs::write(format!("artifacts/pipeline_input.{task_id}.txt"), payload).unwrap();
    db_str
}

// The AnswerQuestion primitive reaches the LLM through
// DefaultLlm::execute_llm, which uses tokio::task::block_in_place — that
// requires a MULTI-THREAD runtime (production runs under #[tokio::main]).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stall_is_diagnosed_not_silent() {
    // R6: the transport is STREAMING; the hang oracle (accept, never send a
    // byte) therefore trips the IDLE timeout — no chunks for the whole
    // window. The diagnostic fact under test is unchanged: an explicit
    // "TIMED OUT after Ns" attribution + a kernel-owned STALL_DETECTED
    // event, never a silent stall. R7: stall-retries are disabled here so
    // the single-attempt diagnostic semantics stay exact.
    std::env::set_var("DAK_LLM_IDLE_TIMEOUT_SECS", "2");
    std::env::set_var("DAK_LLM_REQUEST_TIMEOUT_SECS", "2");
    std::env::set_var("DAK_LLM_STALL_RETRIES", "0");
    std::env::set_var("MLX_LIFECYCLE", "off");
    std::env::set_var("OPENAI_API_KEY", "mlx-local");
    std::env::set_var("OPENAI_MODEL", "/nonexistent/r4-stall-model");
    for purpose in [
        "OPENAI_MODEL_TASK_PLANNING",
        "OPENAI_MODEL_CODING_ASSISTANT",
        "OPENAI_MODEL_CRITIC",
        "OPENAI_MODEL_VERIFIER",
        "OPENAI_MODEL_FINALIZER",
    ] {
        std::env::set_var(purpose, "/nonexistent/r4-stall-model");
    }

    let (base_url, _oracle) = hanging_endpoint();
    std::env::set_var("OPENAI_BASE_URL", &base_url);

    let task_id = unique("r4-stall");
    let db = create_question_task(&task_id, "Сколько будет 2 умножить на 3?");

    let started = Instant::now();
    let err = execute_effects(&db, &task_id)
        .expect_err("a hanging endpoint must fail the step, never complete it");
    let elapsed = started.elapsed();

    // (1) Explicit timeout attribution with the configured window.
    let msg = err.to_string();
    assert!(
        msg.contains("TIMED OUT after 2s"),
        "stall reason lacks explicit attribution: {msg}"
    );
    assert!(
        !msg.contains("error sending request for url"),
        "opaque transport wording survived: {msg}"
    );

    // (3) Fast-fail: the stall costs ONE timeout window, not 3x retried
    // silence (the pre-R4 worst path).
    assert!(
        elapsed < Duration::from_secs(30),
        "stall took {:?} — silent retry multiplication?",
        elapsed
    );

    // (2) Kernel-owned STALL_DETECTED event in the event store.
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db).unwrap();
    let stall_rows: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT payload FROM event_log WHERE event_type = 'STALL_DETECTED'")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    assert_eq!(stall_rows.len(), 1, "exactly one STALL_DETECTED event");
    let stall: serde_json::Value = serde_json::from_str(&stall_rows[0]).unwrap();
    assert_eq!(stall["task_id"], task_id.as_str());
    assert_eq!(stall["elapsed_secs"], 2);
    assert_eq!(stall["llm_calls"], 0);
    assert_eq!(stall["last_known_state"], "pending");
    assert!(stall["reason"]
        .as_str()
        .unwrap()
        .contains("TIMED OUT after 2s"));

    // The task must NOT be completed.
    let state = deterministic_ai_kernel::providers::storage_for(&db)
        .task_state(&task_id)
        .expect("task state readable");
    assert!(
        !matches!(
            state,
            deterministic_ai_kernel::kernel_types::TaskState::Completed
        ),
        "a stalled task must never complete, got {state:?}"
    );

    // Cleanup.
    let _ = std::fs::remove_file(format!("artifacts/pipeline_input.{task_id}.txt"));
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(format!("{db}-wal"));
    let _ = std::fs::remove_file(format!("{db}-shm"));
}
