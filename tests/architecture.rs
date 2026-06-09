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
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") {
                let content = fs::read_to_string(&path).unwrap();
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
