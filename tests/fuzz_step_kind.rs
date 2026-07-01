/// Fuzz §2 — StepKind parser boundary tests
///
/// Covers TODO Phase 1 §2:
///   - malformed inputs never panic, always return None / empty vec
///   - duplicate-heavy sequences normalize correctly
///   - random invalid sequences: system rejects or normalizes, never diverges
use proptest::prelude::*;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs;

fn unique_db(label: &str) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir()
        .join(format!("dak_fuzz_{}_{}.db", label, nanos))
        .display().to_string()
}

fn cleanup(db: &str) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}

/// Run emit-bias-artifact with arbitrary step strings.
/// Must either succeed (exit 0) or reject gracefully (exit 1, no panic/signal).
fn sanitize(s: &str) -> String { s.replace('\0', "") }

fn run_emit(db: &str, task: &str, step: &str, steps: &[String]) -> std::process::Output {
    let mut args = vec!["emit-bias-artifact", task, step];
    let sanitized: Vec<String> = steps.iter().map(|s| sanitize(s)).collect();
    let step_refs: Vec<&str> = sanitized.iter().map(String::as_str).collect();
    args.extend_from_slice(&step_refs);
    Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(&args)
        .output()
        .expect("failed to spawn kernel")
}

// ---------------------------------------------------------------------------
// Property 1: malformed step strings never cause a crash (no signal/panic)
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn fuzz_malformed_steps_never_crash(
        steps in prop::collection::vec(
            // arbitrary unicode strings, including empty, whitespace, control chars
            ".*",
            0..=8,
        ),
        task_suffix in "[a-z]{4}",
    ) {
        let task = format!("task-fuzz-mal-{task_suffix}");
        let db = unique_db(&task);
        cleanup(&db);

        let out = run_emit(&db, &task, "step-fuzz", &steps);

        // Must not be killed by signal (no panic/OOM/segfault)
        // exit code 0 (accepted) or 1 (rejected) are both fine
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            prop_assert!(
                out.status.signal().is_none(),
                "process killed by signal for steps={steps:?}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // If it succeeded, the DB must return a valid JSON payload
        if out.status.success() {
            let list = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
                .env("KERNEL_DB_PATH", &db)
                .args(["latest-bias-artifact", &task, "step-fuzz"])
                .output()
                .expect("failed to spawn kernel");

            if list.status.success() {
                let stdout = String::from_utf8(list.stdout).unwrap();
                if let Some(line) = stdout.lines().next() {
                    let cols: Vec<&str> = line.splitn(6, '\t').collect();
                    if cols.len() == 6 {
                        let parsed: Result<serde_json::Value, _> = serde_json::from_str(cols[5]);
                        prop_assert!(parsed.is_ok(), "invalid JSON in payload: {}", cols[5]);
                    }
                }
            }
        }

        cleanup(&db);
    }
}

// ---------------------------------------------------------------------------
// Property 2: only valid StepKind names are accepted; unknown names are skipped
// ---------------------------------------------------------------------------

const VALID: &[&str] = &[
    "TightenPlannerPrompt", "NormalizePlannerOutput", "AddLlmFallbackHandling",
    "AddPlannerTestCoverage", "ValidatePlannerOutput",
    "AnalyzeTask", "PlanExecution", "ExecuteChanges",
    "ReadRepository", "LocateBug", "PatchCode", "RunTests", "ValidatePatch",
];

const INVALID: &[&str] = &[
    "", " ", "\t", "\n", "analyzeTASK", "ANALYZETASK", "analyze_task",
    "analyze task", "RunTest", "ExecuteChange", "unknown", "null", "None",
    "0", "true", "{}",  "[]", "\"AnalyzeTask\"",
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn fuzz_mixed_valid_invalid_steps_weights_only_for_valid(
        valid_indices   in prop::collection::vec(0usize..VALID.len(),   1..=4),
        invalid_indices in prop::collection::vec(0usize..INVALID.len(), 1..=4),
        task_suffix in "[a-z]{4}",
    ) {
        // interleave valid and invalid
        let mut mixed: Vec<String> = Vec::new();
        for (v, i) in valid_indices.iter().zip(invalid_indices.iter()) {
            mixed.push(VALID[*v].to_string());
            mixed.push(INVALID[*i].to_string());
        }

        let task = format!("task-fuzz-mix-{task_suffix}");
        let db = unique_db(&task);
        cleanup(&db);

        let out = run_emit(&db, &task, "step-mix", &mixed);

        // If accepted: weights must only contain valid StepKind names
        if out.status.success() {
            let list = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
                .env("KERNEL_DB_PATH", &db)
                .args(["latest-bias-artifact", &task, "step-mix"])
                .output().unwrap();

            if list.status.success() {
                let stdout = String::from_utf8(list.stdout).unwrap();
                if let Some(line) = stdout.lines().next() {
                    let cols: Vec<&str> = line.splitn(6, '\t').collect();
                    if cols.len() == 6 {
                        let payload: serde_json::Value =
                            serde_json::from_str(cols[5]).expect("invalid JSON");
                        if let Some(weights) = payload["weights"].as_object() {
                            for key in weights.keys() {
                                prop_assert!(
                                    VALID.contains(&key.as_str()),
                                    "invalid StepKind in weights: {key}"
                                );
                            }
                        }
                    }
                }
            }
        }

        cleanup(&db);
    }
}

// ---------------------------------------------------------------------------
// Property 3: duplicate-heavy sequences → weights deduplicated, no drift
// ---------------------------------------------------------------------------
proptest! {
    #![proptest_config(ProptestConfig::with_cases(60))]

    #[test]
    fn fuzz_duplicate_heavy_sequences_normalize(
        base in prop::sample::select(VALID),
        repeat_count in 2usize..=20,
        task_suffix in "[a-z]{4}",
    ) {
        // e.g. ["AnalyzeTask", "AnalyzeTask", ..., "AnalyzeTask"]
        let steps: Vec<String> = std::iter::repeat(base.to_string()).take(repeat_count).collect();

        let task = format!("task-fuzz-dup-{task_suffix}");
        let db = unique_db(&task);
        cleanup(&db);

        let out = run_emit(&db, &task, "step-dup", &steps);

        if out.status.success() {
            let list = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
                .env("KERNEL_DB_PATH", &db)
                .args(["latest-bias-artifact", &task, "step-dup"])
                .output().unwrap();

            if list.status.success() {
                let stdout = String::from_utf8(list.stdout).unwrap();
                if let Some(line) = stdout.lines().next() {
                    let cols: Vec<&str> = line.splitn(6, '\t').collect();
                    if cols.len() == 6 {
                        let payload: serde_json::Value =
                            serde_json::from_str(cols[5]).expect("invalid JSON");
                        let weights = payload["weights"].as_object().expect("weights must be object");
                        // Deduplicated: only one entry for `base`
                        prop_assert_eq!(
                            weights.len(), 1,
                            "expected 1 weight entry for duplicate input {}×{}, got {:?}", base, repeat_count,
                            weights.keys().collect::<Vec<_>>()
                        );
                        prop_assert!(weights.contains_key(base));
                    }
                }
            }
        }

        cleanup(&db);
    }
}
