pub fn with_mock_lm_backend<F: FnOnce()>(f: F) {
    let prev = std::env::var("DAK_LM_BACKEND").ok();
    std::env::set_var("DAK_LM_BACKEND", "mock");
    f();
    match prev {
        Some(v) => std::env::set_var("DAK_LM_BACKEND", v),
        None => std::env::remove_var("DAK_LM_BACKEND"),
    }
}
