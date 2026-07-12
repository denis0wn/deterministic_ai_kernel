use std::fs;

/// Verify that kernel-core modules do not import domain-specific workflow logic.
///
/// exec_spec, execution_abi, execution_identity, kernel_error, kernel_types
/// must remain domain-agnostic.
fn read_file(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn has_import(content: &str, module: &str) -> bool {
    content.contains(&format!("crate::{}", module))
        || content.contains(&format!("super::{}", module))
        || content.contains(&format!("mod {};", module))
        || content.contains(&format!("use {}::", module))
        || content.contains(&format!("use crate::{}::", module))
}

// ── Kernel Core Isolation ──────────────────────────────────────────────────

#[test]
fn exec_spec_must_not_import_runtime() {
    let content = read_file("src/exec_spec.rs");
    assert!(
        !has_import(&content, "scheduler"),
        "exec_spec must not import scheduler"
    );
    assert!(
        !has_import(&content, "worker"),
        "exec_spec must not import worker"
    );
    assert!(
        !has_import(&content, "llm"),
        "exec_spec must not import llm"
    );
    assert!(
        !has_import(&content, "embeddings"),
        "exec_spec must not import embeddings"
    );
}

#[test]
fn execution_abi_must_not_import_runtime() {
    let content = read_file("src/execution_abi.rs");
    assert!(
        !has_import(&content, "scheduler"),
        "execution_abi must not import scheduler"
    );
    assert!(
        !has_import(&content, "worker"),
        "execution_abi must not import worker"
    );
    assert!(
        !has_import(&content, "llm"),
        "execution_abi must not import llm"
    );
    assert!(
        !has_import(&content, "event_bus"),
        "execution_abi must not import event_bus"
    );
}

#[test]
fn execution_identity_must_not_import_runtime() {
    let content = read_file("src/execution_identity.rs");
    assert!(
        !has_import(&content, "scheduler"),
        "execution_identity must not import scheduler"
    );
    assert!(
        !has_import(&content, "worker"),
        "execution_identity must not import worker"
    );
    assert!(
        !has_import(&content, "event_bus"),
        "execution_identity must not import event_bus"
    );
    assert!(
        !has_import(&content, "workflow"),
        "execution_identity must not import workflow"
    );
}

#[test]
fn kernel_error_must_not_import_runtime() {
    let content = read_file("src/kernel_error.rs");
    assert!(
        !has_import(&content, "scheduler"),
        "kernel_error must not import scheduler"
    );
    assert!(
        !has_import(&content, "worker"),
        "kernel_error must not import worker"
    );
    assert!(
        !has_import(&content, "event_bus"),
        "kernel_error must not import event_bus"
    );
    assert!(
        !has_import(&content, "workflow"),
        "kernel_error must not import workflow"
    );
    assert!(
        !has_import(&content, "llm"),
        "kernel_error must not import llm"
    );
}

#[test]
fn providers_must_not_import_scheduler_or_worker() {
    let content = read_file("src/providers/mod.rs");
    assert!(
        !has_import(&content, "scheduler"),
        "providers must not import scheduler"
    );
    assert!(
        !has_import(&content, "worker"),
        "providers must not import worker"
    );
    assert!(
        !has_import(&content, "event_bus"),
        "providers must not import event_bus"
    );
}

// ── No Hidden Randomness in Kernel Core ────────────────────────────────────

#[test]
fn kernel_core_has_no_hidden_randomness() {
    let core_files = [
        "src/exec_spec.rs",
        "src/execution_abi.rs",
        "src/execution_identity.rs",
        "src/kernel_error.rs",
        "src/kernel_types.rs",
    ];

    let forbidden_patterns = ["thread_rng", "Uuid::new_v4", "rand::random", "OsRng"];

    for file in &core_files {
        let content = read_file(file);
        for pattern in &forbidden_patterns {
            assert!(
                !content.contains(pattern),
                "{} contains forbidden randomness pattern: {}",
                file,
                pattern
            );
        }
    }
}

// ── No Direct Filesystem in Kernel Core ────────────────────────────────────

#[test]
fn kernel_core_has_no_direct_filesystem() {
    let core_files = [
        "src/exec_spec.rs",
        "src/execution_abi.rs",
        "src/execution_identity.rs",
        "src/kernel_error.rs",
        "src/kernel_types.rs",
    ];

    let forbidden = ["std::fs::", "File::open", "File::create"];

    for file in &core_files {
        let content = read_file(file);
        for pattern in &forbidden {
            assert!(
                !content.contains(pattern),
                "{} contains forbidden filesystem access: {}",
                file,
                pattern
            );
        }
    }
}

// ── No Direct Network in Kernel Core ───────────────────────────────────────

#[test]
fn kernel_core_has_no_direct_network() {
    let core_files = [
        "src/exec_spec.rs",
        "src/execution_abi.rs",
        "src/execution_identity.rs",
        "src/kernel_error.rs",
        "src/kernel_types.rs",
    ];

    let forbidden = ["reqwest::", "hyper::", "TcpStream"];

    for file in &core_files {
        let content = read_file(file);
        for pattern in &forbidden {
            assert!(
                !content.contains(pattern),
                "{} contains forbidden network access: {}",
                file,
                pattern
            );
        }
    }
}
