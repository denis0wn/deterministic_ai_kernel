use serde_json::{json, Value};
use std::net::TcpListener;
use std::io::{BufRead, BufReader, Write};

#[tokio::test]
async fn test_native_inference_contract_flow() {
    // 1. Bind to a random port
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_addr = listener.local_addr().unwrap();
    let port = local_addr.port();

    // 2. Spawn mock daemon task in a separate OS thread to avoid single-thread deadlocks
    let _handle = std::thread::spawn(move || {
        println!("[test-server] Mock server starting on port {}", port);
        while let Ok((mut socket, addr)) = listener.accept() {
            println!("[test-server] Mock server accepted connection from {}", addr);
            let mut reader = BufReader::new(&socket);
            let mut line = String::new();
            if let Ok(n) = reader.read_line(&mut line) {
                println!("[test-server] Mock server read {} bytes: {:?}", n, line);
                if n == 0 { continue; }
                if let Ok(req) = serde_json::from_str::<Value>(&line) {
                    if let Some(method) = req.get("method").and_then(|v| v.as_str()) {
                        println!("[test-server] Mock server method: {}", method);
                        if method == "ping" {
                            let resp = json!({
                                "status": "ok",
                                "model": "mock-model"
                            });
                            let mut payload = serde_json::to_string(&resp).unwrap();
                            payload.push('\n');
                            let _ = socket.write_all(payload.as_bytes());
                            let _ = socket.flush();
                            println!("[test-server] Mock server sent ping response");
                        } else if method == "generate" {
                            let params = req.get("params").unwrap();
                            let messages = params.get("messages").unwrap().as_array().unwrap();
                            assert_eq!(messages.len(), 2);
                            let resp = json!({
                                "status": "ok",
                                "text": "final channel extraction output: hello world"
                            });
                            let mut payload = serde_json::to_string(&resp).unwrap();
                            payload.push('\n');
                            let _ = socket.write_all(payload.as_bytes());
                            let _ = socket.flush();
                            println!("[test-server] Mock server sent generate response");
                        }
                    }
                }
            }
        }
    });

    // 3. Temporarily set runtime configuration pointing to our mock daemon
    let config = json!({
        "provider": "mlx",
        "host": "127.0.0.1",
        "port": port,
        "default_model": "mock-model",
        "auto_start": false,
        "venv_path": ".venv-mlx",
        "startup_timeout_secs": 5,
        "health_check_interval_ms": 100
    });
    
    let config_path = "config/runtime.json";
    let prev_config = std::fs::read_to_string(config_path).ok();
    
    std::fs::create_dir_all("config").unwrap();
    std::fs::write(config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

    let prev_backend = std::env::var("DAK_LM_BACKEND").ok();
    std::env::remove_var("DAK_LM_BACKEND");

    // Write current PID to simulate a running process
    let pid_path = "runtime/mlx.pid";
    let prev_pid = std::fs::read_to_string(pid_path).ok();
    std::fs::create_dir_all("runtime").unwrap();
    std::fs::write(pid_path, std::process::id().to_string()).unwrap();

    // 4. Verify native inference health checks succeed
    let mgr = deterministic_ai_kernel::runtime_manager::RuntimeManager::load().unwrap();
    let status = mgr.status().unwrap();
    assert_eq!(status.status, deterministic_ai_kernel::runtime_manager::RuntimeStatus::Running);
    assert_eq!(status.loaded_model.as_deref(), Some("mock-model"));

    // 5. Verify the entire generation and channel extraction flow
    let response = deterministic_ai_kernel::llm::chat_with_role(
        "coding_assistant",
        "You are a mock coding assistant.",
        "hello"
    ).await.unwrap();

    // Verify final channel extraction extracted the correct inner text response
    assert_eq!(response, "final channel extraction output: hello world");

    // Restore environment
    if let Some(pid) = prev_pid {
        std::fs::write(pid_path, pid).unwrap();
    } else {
        let _ = std::fs::remove_file(pid_path);
    }
    if let Some(cfg) = prev_config {
        std::fs::write(config_path, cfg).unwrap();
    } else {
        let _ = std::fs::remove_file(config_path);
    }
    if let Some(bg) = prev_backend {
        std::env::set_var("DAK_LM_BACKEND", bg);
    }
}
