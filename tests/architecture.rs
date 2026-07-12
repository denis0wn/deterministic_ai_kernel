use std::fs;
use std::path::Path;

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

// ── existing boundary tests (preserved) ──────────────────────────────────────

#[test]
fn scheduler_is_selection_only() {
    let content = read_file("src/scheduler.rs");
    assert!(
        !has_import(&content, "worker"),
        "scheduler must not import worker"
    );
    assert!(
        !has_import(&content, "llm"),
        "scheduler must not import llm"
    );
    assert!(
        !has_import(&content, "embeddings"),
        "scheduler must not import embeddings"
    );
    assert!(
        !has_import(&content, "execution"),
        "scheduler must not import execution"
    );
    assert!(
        !has_import(&content, "lm_control"),
        "scheduler must not import lm_control"
    );
}

#[test]
fn worker_is_ownership_only() {
    let content = read_file("src/worker.rs");
    assert!(
        !has_import(&content, "scheduler"),
        "worker must not import scheduler"
    );
    assert!(!has_import(&content, "llm"), "worker must not import llm");
    assert!(
        !has_import(&content, "embeddings"),
        "worker must not import embeddings"
    );
    assert!(
        !has_import(&content, "execution"),
        "worker must not import execution"
    );
    assert!(
        !has_import(&content, "lm_control"),
        "worker must not import lm_control"
    );
}

#[test]
fn execution_is_execution_only() {
    let dir = Path::new("src/execution");
    if dir.exists() {
        for entry in fs::read_dir(dir).expect("test failure") {
            let path = entry.expect("test failure").path();
            if path.extension().is_some_and(|e| e == "rs") {
                let content = fs::read_to_string(&path).expect("test failure");
                assert!(
                    !has_import(&content, "scheduler"),
                    "{:?} must not import scheduler",
                    path
                );
                assert!(
                    !has_import(&content, "worker"),
                    "{:?} must not import worker",
                    path
                );
                assert!(
                    !has_import(&content, "lm_control"),
                    "{:?} must not import lm_control",
                    path
                );
            }
        }
    }
}

#[test]
fn llm_and_embeddings_are_external_only() {
    for file in ["src/llm.rs", "src/embeddings.rs"] {
        let content = read_file(file);
        assert!(
            !has_import(&content, "scheduler"),
            "{} must not import scheduler",
            file
        );
        assert!(
            !has_import(&content, "worker"),
            "{} must not import worker",
            file
        );
        assert!(
            !has_import(&content, "execution"),
            "{} must not import execution",
            file
        );
        assert!(
            !has_import(&content, "workflow"),
            "{} must not import workflow",
            file
        );
    }
}

#[test]
fn event_bus_is_persistence_only() {
    let content = read_file("src/event_bus.rs");
    assert!(
        !has_import(&content, "scheduler"),
        "event_bus must not import scheduler"
    );
    assert!(
        !has_import(&content, "worker"),
        "event_bus must not import worker"
    );
    assert!(
        !has_import(&content, "execution"),
        "event_bus must not import execution"
    );
    assert!(
        !has_import(&content, "llm"),
        "event_bus must not import llm"
    );
    assert!(
        !has_import(&content, "embeddings"),
        "event_bus must not import embeddings"
    );
    assert!(
        !has_import(&content, "workflow"),
        "event_bus must not import workflow"
    );
}

// ── Phase 2 #6: domain→interface boundary (new) ──────────────────────────────
//
// Rule: domain layer (lm_control, embeddings, model_registry, model_manifest,
// llm) MUST NOT import interface layer (cli_json, api).
// Violation found in lm_control.rs before this PR: crate::cli_json calls
// inside print_doctor_json() — now removed.

#[test]
fn domain_must_not_import_cli_json() {
    let domain_files = [
        "src/lm_control.rs",
        "src/embeddings.rs",
        "src/model_registry.rs",
        "src/model_manifest.rs",
        "src/llm.rs",
    ];
    for file in domain_files {
        let content = read_file(file);
        assert!(
            !has_import(&content, "cli_json"),
            "{} must not import cli_json (domain→interface violation)",
            file
        );
    }
}

#[test]
fn domain_must_not_import_api() {
    let domain_files = [
        "src/lm_control.rs",
        "src/embeddings.rs",
        "src/model_registry.rs",
        "src/model_manifest.rs",
        "src/llm.rs",
    ];
    for file in domain_files {
        let content = read_file(file);
        assert!(
            !has_import(&content, "api"),
            "{} must not import api (domain→interface violation)",
            file
        );
    }
}

#[test]
fn kernel_must_not_import_interface() {
    let kernel_files = ["src/kernel_types.rs", "src/snapshot.rs"];
    for file in kernel_files {
        let content = read_file(file);
        assert!(
            !has_import(&content, "cli_json"),
            "{} must not import cli_json (kernel→interface violation)",
            file
        );
        assert!(
            !has_import(&content, "api"),
            "{} must not import api (kernel→interface violation)",
            file
        );
    }
}

#[test]
fn kernel_must_not_import_domain() {
    let kernel_files = ["src/kernel_types.rs", "src/snapshot.rs"];
    for file in kernel_files {
        let content = read_file(file);
        assert!(
            !has_import(&content, "lm_control"),
            "{} must not import lm_control (kernel→domain violation)",
            file
        );
        assert!(
            !has_import(&content, "model_registry"),
            "{} must not import model_registry (kernel→domain violation)",
            file
        );
        assert!(
            !has_import(&content, "embeddings"),
            "{} must not import embeddings (kernel→domain violation)",
            file
        );
    }
}
