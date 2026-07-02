use std::fs;
use std::path::Path;

// Hard-fail if a required source file is missing.
// unwrap_or_default() was unsafe: a deleted file would silently
// pass all boundary assertions (empty string contains nothing).
fn read_file(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Required source file missing: {} ({e})", path))
}

// Read a file that may legitimately not exist yet (migration in progress).
// Returns None when absent so callers can skip or assert presence explicitly.
fn try_read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn has_import(content: &str, module: &str) -> bool {
    content.contains(&format!("crate::{}", module))
        || content.contains(&format!("super::{}", module))
        || content.contains(&format!("mod {};", module))
        || content.contains(&format!("use {}::", module))
        || content.contains(&format!("use crate::{}::", module))
}

// ── existing boundary tests (paths updated to new layout) ────────────────────

#[test]
fn scheduler_is_selection_only() {
    let content = read_file("src/scheduler.rs");
    assert!(!has_import(&content, "worker"),    "scheduler must not import worker");
    assert!(!has_import(&content, "llm"),       "scheduler must not import llm");
    assert!(!has_import(&content, "embeddings"),"scheduler must not import embeddings");
    assert!(!has_import(&content, "execution"), "scheduler must not import execution");
    assert!(!has_import(&content, "lm_control"),"scheduler must not import lm_control");
}

#[test]
fn worker_is_ownership_only() {
    let content = read_file("src/worker.rs");
    assert!(!has_import(&content, "scheduler"), "worker must not import scheduler");
    assert!(!has_import(&content, "llm"),       "worker must not import llm");
    assert!(!has_import(&content, "embeddings"),"worker must not import embeddings");
    assert!(!has_import(&content, "execution"), "worker must not import execution");
    assert!(!has_import(&content, "lm_control"),"worker must not import lm_control");
}

#[test]
fn execution_is_execution_only() {
    let dir = Path::new("src/execution");
    if dir.exists() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") {
                let content = fs::read_to_string(&path).unwrap();
                assert!(!has_import(&content, "scheduler"), "{:?} must not import scheduler", path);
                assert!(!has_import(&content, "worker"),    "{:?} must not import worker", path);
                assert!(!has_import(&content, "lm_control"),"{:?} must not import lm_control", path);
            }
        }
    }
}

#[test]
fn llm_and_embeddings_are_external_only() {
    for file in ["src/llm.rs", "src/embeddings.rs"] {
        let content = read_file(file);
        assert!(!has_import(&content, "scheduler"), "{} must not import scheduler", file);
        assert!(!has_import(&content, "worker"),    "{} must not import worker", file);
        assert!(!has_import(&content, "execution"), "{} must not import execution", file);
        assert!(!has_import(&content, "workflow"),  "{} must not import workflow", file);
    }
}

// event_bus: check canonical new path; legacy path checked via try_read_file
// so the test degrades gracefully during the migration window but never
// silently passes on an absent file once the legacy file is gone.
#[test]
fn event_bus_is_persistence_only() {
    // New canonical location (must exist after commit-A)
    let content = read_file("src/engine/event_bus.rs");
    assert!(!has_import(&content, "scheduler"), "event_bus must not import scheduler");
    assert!(!has_import(&content, "worker"),    "event_bus must not import worker");
    assert!(!has_import(&content, "execution"), "event_bus must not import execution");
    assert!(!has_import(&content, "llm"),       "event_bus must not import llm");
    assert!(!has_import(&content, "embeddings"),"event_bus must not import embeddings");
    assert!(!has_import(&content, "workflow"),  "event_bus must not import workflow");

    // Legacy location: still checked while src/event_bus.rs exists
    if let Some(legacy) = try_read_file("src/event_bus.rs") {
        assert!(!has_import(&legacy, "scheduler"), "legacy event_bus must not import scheduler");
        assert!(!has_import(&legacy, "worker"),    "legacy event_bus must not import worker");
        assert!(!has_import(&legacy, "execution"), "legacy event_bus must not import execution");
        assert!(!has_import(&legacy, "llm"),       "legacy event_bus must not import llm");
        assert!(!has_import(&legacy, "embeddings"),"legacy event_bus must not import embeddings");
        assert!(!has_import(&legacy, "workflow"),  "legacy event_bus must not import workflow");
    }
}

// ── Phase 2 #6: domain→interface boundary ────────────────────────────────────────

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

// kernel/core: check canonical new paths; legacy paths via try_read_file
#[test]
fn kernel_must_not_import_interface() {
    // New canonical locations (must exist after commit-A)
    for file in [
        "src/kernel/core/types.rs",
        "src/kernel/core/snapshot.rs",
        "src/kernel/core/effects.rs",
    ] {
        let content = read_file(file);
        assert!(!has_import(&content, "cli_json"), "{} must not import cli_json (kernel→interface)", file);
        assert!(!has_import(&content, "api"),      "{} must not import api (kernel→interface)", file);
    }

    // Legacy locations: checked while they still exist
    for file in ["src/kernel_types.rs", "src/snapshot.rs"] {
        if let Some(content) = try_read_file(file) {
            assert!(!has_import(&content, "cli_json"), "{} must not import cli_json (kernel→interface)", file);
            assert!(!has_import(&content, "api"),      "{} must not import api (kernel→interface)", file);
        }
    }
}

#[test]
fn kernel_must_not_import_domain() {
    // New canonical locations
    for file in [
        "src/kernel/core/types.rs",
        "src/kernel/core/snapshot.rs",
        "src/kernel/core/effects.rs",
    ] {
        let content = read_file(file);
        assert!(!has_import(&content, "lm_control"),    "{} must not import lm_control (kernel→domain)", file);
        assert!(!has_import(&content, "model_registry"),"{} must not import model_registry (kernel→domain)", file);
        assert!(!has_import(&content, "embeddings"),    "{} must not import embeddings (kernel→domain)", file);
    }

    // Legacy locations
    for file in ["src/kernel_types.rs", "src/snapshot.rs"] {
        if let Some(content) = try_read_file(file) {
            assert!(!has_import(&content, "lm_control"),    "{} must not import lm_control (kernel→domain)", file);
            assert!(!has_import(&content, "model_registry"),"{} must not import model_registry (kernel→domain)", file);
            assert!(!has_import(&content, "embeddings"),    "{} must not import embeddings (kernel→domain)", file);
        }
    }
}

// ── Phase 2B: invariant layer boundary ──────────────────────────────────────
//
// kernel/invariant is allowed ONE upward reference: engine/event_bus.
// It must not touch domain or interface.

#[test]
fn invariant_layer_must_not_import_domain_or_interface() {
    let invariant_dir = Path::new("src/kernel/invariant");
    if !invariant_dir.exists() {
        panic!("Required directory missing: src/kernel/invariant");
    }
    for entry in walkdir(invariant_dir) {
        let content = read_file(entry.to_str().unwrap());
        assert!(!has_import(&content, "lm_control"),   "{:?} (invariant) must not import lm_control",  entry);
        assert!(!has_import(&content, "model_registry"),"{:?} (invariant) must not import model_registry", entry);
        assert!(!has_import(&content, "embeddings"),   "{:?} (invariant) must not import embeddings",  entry);
        assert!(!has_import(&content, "cli_json"),     "{:?} (invariant) must not import cli_json",    entry);
        assert!(!has_import(&content, "api"),          "{:?} (invariant) must not import api",         entry);
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────────

fn walkdir(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.extend(walkdir(&p));
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    out
}
