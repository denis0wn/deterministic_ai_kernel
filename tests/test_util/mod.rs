pub fn with_mock_lm_backend<F: FnOnce()>(f: F) {
    let prev_backend = std::env::var("DAK_LM_BACKEND").ok();
    let prev_mem = std::env::var("DAK_FREE_GB_OVERRIDE").ok();
    std::env::set_var("DAK_LM_BACKEND", "mock");
    std::env::set_var("DAK_FREE_GB_OVERRIDE", "16.0");
    f();
    match prev_backend {
        Some(v) => std::env::set_var("DAK_LM_BACKEND", v),
        None => std::env::remove_var("DAK_LM_BACKEND"),
    }
    match prev_mem {
        Some(v) => std::env::set_var("DAK_FREE_GB_OVERRIDE", v),
        None => std::env::remove_var("DAK_FREE_GB_OVERRIDE"),
    }
}
