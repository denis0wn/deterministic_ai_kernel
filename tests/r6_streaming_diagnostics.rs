//! R6 — streaming client diagnostics (idle timeout + hard cap).
//!
//! A local fake SSE endpoint stands in for the MLX server at the TRANSPORT
//! level only (the kernel's own llm client, retry policy, timeout logic and
//! event emission are the code under test — no kernel logic is mocked).
//!
//! Scenarios proven:
//! 1. a GAP between chunks longer than DAK_LLM_IDLE_TIMEOUT_SECS triggers
//!    the idle timeout with explicit attribution;
//! 2. a CONTINUOUS slow stream (longer than the idle window, chunks always
//!    flowing) completes — long-but-alive generation is waited out, not
//!    misclassified as a hang (the NEW-1 fix);
//! 3. the HARD CAP fires despite active chunks when total duration exceeds
//!    DAK_LLM_REQUEST_TIMEOUT_SECS (truly unbounded generation).

use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Proper HTTP/1.1 chunked-transfer framing — hyper/reqwest reject responses
/// with neither Content-Length nor chunked TE.
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

fn set_llm_env(base_url: &str, idle: &str, cap: &str) {
    std::env::set_var("OPENAI_BASE_URL", base_url);
    std::env::set_var("OPENAI_API_KEY", "r6-local");
    std::env::set_var("OPENAI_MODEL", "/nonexistent/r6-model");
    std::env::set_var("DAK_LLM_IDLE_TIMEOUT_SECS", idle);
    std::env::set_var("DAK_LLM_REQUEST_TIMEOUT_SECS", cap);
    // R7: no stall-retries here — the single-oracle scenarios assert exact
    // first-attempt semantics (recovery-on-fresh-connection is tested in
    // r7_wedge_recovery).
    std::env::set_var("DAK_LLM_STALL_RETRIES", "0");
    std::env::set_var("MLX_LIFECYCLE", "off");
    for purpose in [
        "OPENAI_MODEL_TASK_PLANNING",
        "OPENAI_MODEL_CODING_ASSISTANT",
        "OPENAI_MODEL_CRITIC",
        "OPENAI_MODEL_VERIFIER",
        "OPENAI_MODEL_FINALIZER",
    ] {
        std::env::set_var(purpose, "/nonexistent/r6-model");
    }
}

async fn sse_listen(behavior: SseBehavior) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            // Read the request head first (a real server consumes the
            // request before answering); stops at the blank line.
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match socket.read(&mut byte).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        head.push(byte[0]);
                        if head.ends_with(b"\r\n\r\n") {
                            break;
                        }
                        if head.len() > 16384 {
                            break;
                        }
                    }
                }
            }
            let _ = socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
                )
                .await;
            match behavior {
                SseBehavior::OneChunkThenSilence(chunk) => {
                    send_chunked(&mut socket, chunk.as_bytes()).await;
                    // Never send another byte; hold the socket open.
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                SseBehavior::SlowStream(chunks, interval) => {
                    for chunk in chunks {
                        send_chunked(&mut socket, chunk.as_bytes()).await;
                        tokio::time::sleep(interval).await;
                    }
                    send_chunked(&mut socket, b"data: [DONE]\n\n").await;
                    end_chunked(&mut socket).await;
                    // Graceful half-close: dropping the socket with unread
                    // request bytes in the receive buffer makes the kernel
                    // RST the connection and discard buffered response
                    // tails. shutdown() flushes pending data + FIN.
                    let _ = socket.shutdown().await;
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                SseBehavior::EndlessStream(interval) => {
                    let mut i = 0u64;
                    loop {
                        let line = format!(
                            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"t{i}\"}}}}]}}\n\n"
                        );
                        send_chunked(&mut socket, line.as_bytes()).await;
                        i += 1;
                        tokio::time::sleep(interval).await;
                    }
                }
            }
        }
    });
    format!("http://{addr}/v1")
}

enum SseBehavior {
    OneChunkThenSilence(String),
    SlowStream(Vec<String>, Duration),
    EndlessStream(Duration),
}

fn content_chunk(piece: &str) -> String {
    format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{piece}\"}}}}]}}\n\n")
}

// One async entry point: env vars are process-global, so the scenarios run
// SEQUENTIALLY inside a single test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_idle_vs_alive_vs_hardcap() {
    // ── scenario 1: gap between chunks > idle → idle timeout ───────────
    let base = sse_listen(SseBehavior::OneChunkThenSilence(content_chunk("par"))).await;
    set_llm_env(&base, "2", "60");
    let started = Instant::now();
    let err = deterministic_ai_kernel::llm::chat("You are a test endpoint.", "Say one word.")
        .await
        .expect_err("a silent gap beyond the idle window must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("TIMED OUT after 2s"),
        "idle timeout lacks attribution: {msg}"
    );
    assert!(
        msg.contains("waiting for next chunk"),
        "idle semantics missing: {msg}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "idle stall not fail-fast: {:?}",
        started.elapsed()
    );

    // ── scenario 2: continuous slow stream → completes ─────────────────
    // 10 chunks at 300ms = 3s total: longer than the idle window (1s),
    // alive the whole time. Pre-R6 total-timeout thinking would reject
    // long generations; idle semantics must accept them.
    let base = sse_listen(SseBehavior::SlowStream(
        (0..10).map(|i| content_chunk(&format!("w{i} "))).collect(),
        Duration::from_millis(300),
    ))
    .await;
    set_llm_env(&base, "1", "60");
    let started = Instant::now();
    let ok = deterministic_ai_kernel::llm::chat("You are a test endpoint.", "Stream ten words.")
        .await
        .expect("a continuous slow stream must complete, not time out");
    let elapsed = started.elapsed();
    for i in 0..10 {
        assert!(ok.contains(&format!("w{i}")), "missing chunk w{i}: {ok}");
    }
    assert!(
        elapsed >= Duration::from_millis(2500),
        "stream finished too early ({elapsed:?}) — chunks not consumed?"
    );

    // ── scenario 3: hard cap despite active chunks ─────────────────────
    let base = sse_listen(SseBehavior::EndlessStream(Duration::from_millis(100))).await;
    set_llm_env(&base, "10", "3");
    let started = Instant::now();
    let err = deterministic_ai_kernel::llm::chat("You are a test endpoint.", "Never stop.")
        .await
        .expect_err("an unbounded generation must hit the hard cap");
    let msg = err.to_string();
    assert!(
        msg.contains("HARD_TIMEOUT_EXCEEDED after 3s"),
        "hard cap lacks attribution: {msg}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "hard cap not enforced promptly: {:?}",
        started.elapsed()
    );

    // ── scenario 4: reasoning-only stream (B6 live shape) ──────────────
    // Reasoning models may emit the entire answer in `reasoning` deltas
    // and leave `content` empty. The streaming contract must mirror the
    // non-streaming content.or(reasoning) fallback.
    let base = sse_listen(SseBehavior::SlowStream(
        vec![
            reasoning_chunk("thinking "),
            reasoning_chunk("about "),
            reasoning_chunk("it... "),
            reasoning_chunk("deadlock."),
        ],
        Duration::from_millis(100),
    ))
    .await;
    set_llm_env(&base, "2", "60");
    let ok = deterministic_ai_kernel::llm::chat(
        "You are a test endpoint.",
        "Answer via reasoning only.",
    )
    .await
    .expect("a reasoning-only stream must still produce an answer");
    assert_eq!(ok, "thinking about it... deadlock.");
}

fn reasoning_chunk(piece: &str) -> String {
    format!("data: {{\"choices\":[{{\"delta\":{{\"reasoning\":\"{piece}\"}}}}]}}\n\n")
}
