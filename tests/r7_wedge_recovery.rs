//! R7 — wedge recovery & concurrency diagnostics.
//!
//! Fake SSE servers stand in at the TRANSPORT level only (the kernel's own
//! llm client, stall policy and retry decisions are the code under test).
//!
//! Scenarios proven:
//! 1. an idle stall on one connection is aborted and RETRIED ON A FRESH
//!    connection, which then succeeds (the R7 wedge-recovery policy);
//! 2. the stall-retry budget is BOUNDED — an endpoint where every
//!    connection hangs still fails fast with the idle-timeout diagnostic
//!    (no infinite retry loop);
//! 3. with DAK_LLM_MAX_CONCURRENT=2 two concurrent requests are both
//!    served (server-side concurrency was proven live, R7 EXP A; this
//!    proves the kernel client handles it).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique(name: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{name}_{n}_{nanos}")
}

fn set_env(base_url: &str, idle: &str, cap: &str, retries: &str, concurrent: &str) {
    std::env::set_var("OPENAI_BASE_URL", base_url);
    std::env::set_var("OPENAI_API_KEY", "r7-local");
    std::env::set_var("OPENAI_MODEL", "/nonexistent/r7-model");
    std::env::set_var("DAK_LLM_IDLE_TIMEOUT_SECS", idle);
    std::env::set_var("DAK_LLM_REQUEST_TIMEOUT_SECS", cap);
    std::env::set_var("DAK_LLM_STALL_RETRIES", retries);
    std::env::set_var("DAK_LLM_MAX_CONCURRENT", concurrent);
    std::env::set_var("MLX_LIFECYCLE", "off");
    for purpose in [
        "OPENAI_MODEL_TASK_PLANNING",
        "OPENAI_MODEL_CODING_ASSISTANT",
        "OPENAI_MODEL_CRITIC",
        "OPENAI_MODEL_VERIFIER",
        "OPENAI_MODEL_FINALIZER",
    ] {
        std::env::set_var(purpose, "/nonexistent/r7-model");
    }
}

/// Proper HTTP/1.1 chunked framing (hyper rejects length-less responses).
async fn send_chunked(socket: &mut TcpStream, data: &[u8]) {
    let _ = socket
        .write_all(format!("{:x}\r\n", data.len()).as_bytes())
        .await;
    let _ = socket.write_all(data).await;
    let _ = socket.write_all(b"\r\n").await;
    let _ = socket.flush().await;
}

async fn end_chunked(socket: &mut TcpStream) {
    let _ = socket.write_all(b"0\r\n\r\n").await;
    let _ = socket.flush().await;
}

async fn read_request_head(socket: &mut TcpStream) {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match socket.read(&mut byte).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                head.push(byte[0]);
                if head.ends_with(b"\r\n\r\n") || head.len() > 16384 {
                    break;
                }
            }
        }
    }
}

async fn send_headers(socket: &mut TcpStream) {
    let _ = socket
        .write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
        )
        .await;
}

fn content_chunk(piece: &str) -> String {
    format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{piece}\"}}}}]}}\n\n")
}

enum ConnBehavior {
    /// Accept, headers, then silence (idle-stall oracle).
    HangThenSilence,
    /// Accept, headers, stream one content chunk, done.
    Reply(&'static str),
}

/// Serve a queue of per-connection behaviors. Each connection is handled
/// in its OWN task — exactly the property under test: a hung connection
/// must not block the accept loop (mirrors mlx_lm.server EXP A/B).
async fn serve(listener: TcpListener, behaviors: Vec<ConnBehavior>) {
    tokio::spawn(async move {
        for behavior in behaviors {
            if let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    handle_conn(socket, behavior).await;
                });
            }
        }
    });
}

async fn handle_conn(mut socket: TcpStream, behavior: ConnBehavior) {
    read_request_head(&mut socket).await;
    send_headers(&mut socket).await;
    match behavior {
        ConnBehavior::HangThenSilence => {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        ConnBehavior::Reply(text) => {
            send_chunked(&mut socket, content_chunk(text).as_bytes()).await;
            send_chunked(&mut socket, b"data: [DONE]\n\n").await;
            end_chunked(&mut socket).await;
            let _ = socket.shutdown().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

// Env vars are process-global → all scenarios run sequentially in one test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wedge_recovery_and_concurrency() {
    // ── scenario 1: stall → abort → fresh connection succeeds ──────────
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    serve(
        listener,
        vec![
            ConnBehavior::HangThenSilence,
            ConnBehavior::Reply("recovered"),
        ],
    )
    .await;
    set_env(&base, "1", "60", "1", "2");
    let started = Instant::now();
    let ok = deterministic_ai_kernel::llm::chat("You are a test endpoint.", "Hang then recover.")
        .await
        .expect("stall on conn #1 must be retried on a fresh connection");
    assert_eq!(ok, "recovered");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "recovery took {:?} — retry path not engaged?",
        started.elapsed()
    );

    // ── scenario 2: every connection hangs → bounded budget, diagnostic ─
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    serve(
        listener,
        vec![
            ConnBehavior::HangThenSilence,
            ConnBehavior::HangThenSilence,
            ConnBehavior::HangThenSilence,
        ],
    )
    .await;
    set_env(&base, "1", "60", "1", "2");
    let started = Instant::now();
    let err = deterministic_ai_kernel::llm::chat("You are a test endpoint.", "Hang forever.")
        .await
        .expect_err("an endpoint where every connection stalls must still fail");
    let msg = err.to_string();
    assert!(
        msg.contains("TIMED OUT after 1s") && msg.contains("waiting for next chunk"),
        "idle diagnostic lost after retries: {msg}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "retry budget not bounded: {:?}",
        started.elapsed()
    );

    // ── scenario 3: two concurrent requests both served ────────────────
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    serve(
        listener,
        vec![ConnBehavior::Reply("first"), ConnBehavior::Reply("second")],
    )
    .await;
    set_env(&base, "5", "60", "1", "2");
    let (a, b) = tokio::join!(
        deterministic_ai_kernel::llm::chat("You are a test endpoint.", "Request A."),
        deterministic_ai_kernel::llm::chat("You are a test endpoint.", "Request B.")
    );
    let mut results = vec![
        a.expect("concurrent A must complete"),
        b.expect("concurrent B must complete"),
    ];
    results.sort();
    assert_eq!(results, vec!["first".to_string(), "second".to_string()]);
}

#[test]
fn r7_env_parsers_are_clamped_and_defaulted() {
    std::env::remove_var("DAK_LLM_MAX_CONCURRENT");
    std::env::remove_var("DAK_LLM_STALL_RETRIES");
    assert_eq!(deterministic_ai_kernel::llm::llm_max_concurrent(), 1);
    assert_eq!(deterministic_ai_kernel::llm::llm_stall_retries(), 1);
    std::env::set_var("DAK_LLM_MAX_CONCURRENT", "3");
    std::env::set_var("DAK_LLM_STALL_RETRIES", "2");
    assert_eq!(deterministic_ai_kernel::llm::llm_max_concurrent(), 3);
    assert_eq!(deterministic_ai_kernel::llm::llm_stall_retries(), 2);
    std::env::set_var("DAK_LLM_MAX_CONCURRENT", "99");
    std::env::set_var("DAK_LLM_STALL_RETRIES", "99");
    assert_eq!(deterministic_ai_kernel::llm::llm_max_concurrent(), 8);
    assert_eq!(deterministic_ai_kernel::llm::llm_stall_retries(), 3);
    std::env::remove_var("DAK_LLM_MAX_CONCURRENT");
    std::env::remove_var("DAK_LLM_STALL_RETRIES");
}
