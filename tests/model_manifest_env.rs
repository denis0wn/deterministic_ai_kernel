use deterministic_ai_kernel::model_manifest::{current_model_statuses, sync_all_roles};
use std::fs;

struct EnvGuard {
    original: Option<String>,
}

impl EnvGuard {
    fn capture() -> Self {
        Self {
            original: fs::read_to_string(".env").ok(),
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(text) => { let _ = fs::write(".env", text); }
            None => { let _ = fs::remove_file(".env"); }
        }
    }
}

#[test]
fn sync_all_roles_makes_statuses_in_sync() {
    let _guard = EnvGuard::capture();
    let _ = fs::remove_file(".env");

    sync_all_roles().unwrap();
    let rows = current_model_statuses().unwrap();

    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r.in_sync));
    assert!(rows.iter().all(|r| r.env_model.as_deref() == Some(r.manifest_model.as_str())));
}
