use serde_json::json;

#[tokio::test]
async fn test_embeddings_routing_contract_flow() {
    let config_path = "config/runtime.json";
    let prev_config = std::fs::read_to_string(config_path).ok();
    let prev_backend = std::env::var("DAK_LM_BACKEND").ok();

    // ── Test Case 1: Mock Success ────────────────────────────────────────────
    {
        let config = json!({
            "provider": "mlx",
            "host": "127.0.0.1",
            "port": 54158,
            "default_model": "mock-model",
            "auto_start": false,
            "venv_path": ".venv-mlx",
            "startup_timeout_secs": 5,
            "health_check_interval_ms": 100,
            "embeddings": {
                "provider": "mock",
                "model": "text-embedding-nomic-embed-text-v1.5",
                "endpoint": "http://127.0.0.1:9999",
                "health_check_url": "http://127.0.0.1:9999"
            }
        });

        std::fs::create_dir_all("config").unwrap();
        std::fs::write(config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
        std::env::set_var("DAK_LM_BACKEND", "mock");

        // Verify embed_text succeeds with mock vectors (dimension 1536)
        let res = deterministic_ai_kernel::embeddings::embed_text("hello").await;
        assert!(res.is_ok(), "Mock embed_text failed: {:?}", res.err());
        let vec = res.unwrap();
        assert_eq!(vec.len(), 1536);

        // Verify doctor report returns AVAILABLE status
        let report = deterministic_ai_kernel::lm_control::doctor().unwrap();
        assert_eq!(report.embedding_status, "AVAILABLE");
    }

    // ── Test Case 2: Not Configured ──────────────────────────────────────────
    {
        let config = json!({
            "provider": "mlx",
            "host": "127.0.0.1",
            "port": 54158,
            "default_model": "mock-model",
            "auto_start": false,
            "venv_path": ".venv-mlx",
            "startup_timeout_secs": 5,
            "health_check_interval_ms": 100
        });

        std::fs::write(config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
        std::env::set_var("DAK_LM_BACKEND", "mock");

        // Verify doctor reports NOT_CONFIGURED
        let report = deterministic_ai_kernel::lm_control::doctor().unwrap();
        assert_eq!(report.embedding_status, "NOT_CONFIGURED");
    }

    // ── Test Case 3: Blocked (unreachable endpoint) ───────────────────────────
    {
        let config = json!({
            "provider": "mlx",
            "host": "127.0.0.1",
            "port": 54158,
            "default_model": "mock-model",
            "auto_start": false,
            "venv_path": ".venv-mlx",
            "startup_timeout_secs": 5,
            "health_check_interval_ms": 100,
            "embeddings": {
                "provider": "nomic",
                "model": "text-embedding-nomic-embed-text-v1.5",
                "endpoint": "http://127.0.0.1:65431", // unreachable port
                "health_check_url": "http://127.0.0.1:65431"
            }
        });

        std::fs::write(config_path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
        std::env::remove_var("DAK_LM_BACKEND");

        // Verify doctor reports BLOCKED
        let report = deterministic_ai_kernel::lm_control::doctor().unwrap();
        assert_eq!(report.embedding_status, "BLOCKED");
        assert!(report.embedding_reason.is_some());
    }

    // ── Cleanup ──────────────────────────────────────────────────────────────
    if let Some(cfg) = prev_config {
        std::fs::write(config_path, cfg).unwrap();
    } else {
        let _ = std::fs::remove_file(config_path);
    }
    if let Some(bg) = prev_backend {
        std::env::set_var("DAK_LM_BACKEND", bg);
    } else {
        std::env::remove_var("DAK_LM_BACKEND");
    }
}
