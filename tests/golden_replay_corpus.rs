use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize)]
struct Scenario {
    name: String,
    commands: Vec<Vec<String>>,
    expect_substrings: Vec<String>,
    expected_artifact_refs: Vec<String>,
    #[serde(default)]
    expect_failure: bool,
    #[serde(default)]
    expect_failure_substrings: Vec<String>,
}

fn unique_db_path(test_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "deterministic_ai_kernel_{}_{}.db",
        test_name, nanos
    ))
}

fn cleanup(db: &Path) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{}-wal", db.display()));
    let _ = fs::remove_file(format!("{}-shm", db.display()));
}

fn run_fail(db: &Path, args: &[String]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .expect("failed to run kernel");

    assert!(
        !out.status.success(),
        "expected failure: {:?}\nstdout=\n{}\nstderr=\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn run(db: &Path, args: &[String]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .expect("failed to run kernel");

    assert!(
        out.status.success(),
        "command failed: {:?}\nstdout=\n{}\nstderr=\n{}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    String::from_utf8_lossy(&out.stdout).to_string()
}

#[test]
fn golden_replay_corpus_scenarios_match_contracts() {
    let corpus_dir = Path::new("tests/replay_corpus");
    let mut entries: Vec<PathBuf> = fs::read_dir(corpus_dir)
        .expect("read corpus dir")
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();

    entries.sort();

    assert!(!entries.is_empty(), "expected replay corpus scenarios");

    for path in entries {
        let raw = fs::read_to_string(&path).expect("read scenario");
        let scenario: Scenario = serde_json::from_str(&raw).expect("parse scenario json");

        let db = unique_db_path(&scenario.name);
        cleanup(&db);

        let mut combined = String::new();
        for (idx, command) in scenario.commands.iter().enumerate() {
            let is_last = idx + 1 == scenario.commands.len();
            let output = if scenario.expect_failure && is_last {
                run_fail(&db, command)
            } else {
                run(&db, command)
            };
            combined.push_str(&output);
            combined.push('\n');
        }

        for needle in &scenario.expect_substrings {
            assert!(
                combined.contains(needle),
                "scenario={} missing substring {:?}\noutput=\n{}",
                scenario.name,
                needle,
                combined
            );
        }

        for artifact in &scenario.expected_artifact_refs {
            assert!(
                combined.contains(artifact),
                "scenario={} missing artifact ref {:?}\noutput=\n{}",
                scenario.name,
                artifact,
                combined
            );
        }

        for needle in &scenario.expect_failure_substrings {
            assert!(
                combined.contains(needle),
                "scenario={} missing failure substring {:?}\noutput=\n{}",
                scenario.name,
                needle,
                combined
            );
        }

        cleanup(&db);
    }
}
