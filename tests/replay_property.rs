use proptest::prelude::*;
use serde_json::Value;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_db(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test failure")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("dak_prop_{}_{}.db", label, nanos))
        .display()
        .to_string()
}

fn cleanup(db: &str) {
    let _ = fs::remove_file(db);
    let _ = fs::remove_file(format!("{db}-wal"));
    let _ = fs::remove_file(format!("{db}-shm"));
}

fn run_ok(db: &str, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_deterministic_ai_kernel"))
        .env("KERNEL_DB_PATH", db)
        .args(args)
        .output()
        .expect("failed to spawn kernel");
    assert!(
        out.status.success(),
        "cmd {:?} failed\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("test failure")
}

fn emit_and_fetch(db: &str, task: &str, step: &str, steps: &[&str]) -> Value {
    let mut args = vec!["emit-bias-artifact", task, step];
    args.extend_from_slice(steps);
    let _ = run_ok(db, &args);
    let stdout = run_ok(db, &["latest-bias-artifact", task, step]);
    let line = stdout.lines().next().expect("expected artifact row");
    let cols: Vec<&str> = line.splitn(6, '\t').collect();
    assert_eq!(cols.len(), 6);
    serde_json::from_str(cols[5]).expect("invalid JSON")
}

const STEP_KINDS: &[&str] = &[
    "AnalyzeTask",
    "PlanExecution",
    "ExecuteChanges",
    "RunTests",
    "ReadRepository",
    "LocateBug",
    "PatchCode",
    "ValidatePatch",
];

fn arb_step() -> impl Strategy<Value = &'static str> {
    prop::sample::select(STEP_KINDS)
}
fn arb_steps(min: usize, max: usize) -> impl Strategy<Value = Vec<&'static str>> {
    prop::collection::vec(arb_step(), min..=max)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    #[test]
    fn prop_repeat_emit_is_idempotent(steps in arb_steps(1, 6), task_suffix in "[a-z]{4}") {
        let task = format!("task-idem-{task_suffix}");
        let db = unique_db(&task); cleanup(&db);
        let step_refs: Vec<&str> = steps.iter().map(|s| s.as_ref()).collect();
        let first  = emit_and_fetch(&db, &task, "step-idem", &step_refs);
        let second = emit_and_fetch(&db, &task, "step-idem", &step_refs);
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(&first["version"], "v1");
        prop_assert_eq!(&first["seed"], 0);
        cleanup(&db);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn prop_snapshot_restore_preserves_payload(steps in arb_steps(1, 8), task_suffix in "[a-z]{4}") {
        let task = format!("task-snap-{task_suffix}");
        let db = unique_db(&task); cleanup(&db);
        let step_refs: Vec<&str> = steps.iter().map(|s| s.as_ref()).collect();
        let before = emit_and_fetch(&db, &task, "step-snap", &step_refs);
        let _ = run_ok(&db, &["snapshot", &task]);
        let _ = run_ok(&db, &["restore",  &task]);
        let after = emit_and_fetch(&db, &task, "step-snap", &step_refs);
        prop_assert_eq!(&before, &after);
        cleanup(&db);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]
    #[test]
    fn prop_step_order_does_not_affect_weights(
        indices in prop::collection::hash_set(0usize..STEP_KINDS.len(), 2..=5),
        task_suffix in "[a-z]{4}",
    ) {
        let mut idx_vec: Vec<usize> = indices.into_iter().collect();
        idx_vec.sort_unstable();
        let ordered:  Vec<&str> = idx_vec.iter().map(|&i| STEP_KINDS[i]).collect();
        let reversed: Vec<&str> = idx_vec.iter().rev().map(|&i| STEP_KINDS[i]).collect();
        let db_a = unique_db(&format!("ord-a-{task_suffix}"));
        let db_b = unique_db(&format!("ord-b-{task_suffix}"));
        cleanup(&db_a); cleanup(&db_b);
        let pa = emit_and_fetch(&db_a, &format!("task-a-{task_suffix}"), "step-ord", &ordered);
        let pb = emit_and_fetch(&db_b, &format!("task-b-{task_suffix}"), "step-ord", &reversed);
        prop_assert_eq!(&pa["weights"], &pb["weights"]);
        cleanup(&db_a); cleanup(&db_b);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    #[test]
    fn prop_batch_structural_invariants(steps in arb_steps(1, 5), task_suffix in "[a-z0-9]{6}") {
        let task = format!("task-batch-{task_suffix}");
        let db = unique_db(&task); cleanup(&db);
        let step_refs: Vec<&str> = steps.iter().map(|s| s.as_ref()).collect();
        let payload = emit_and_fetch(&db, &task, "step-batch", &step_refs);
        prop_assert_eq!(&payload["version"], "v1");
        prop_assert_eq!(&payload["seed"], 0);
        let weights = payload["weights"].as_object().expect("weights must be object");
        prop_assert!(!weights.is_empty());
        let unique_steps: std::collections::HashSet<&str> = step_refs.iter().copied().collect();
        for s in &unique_steps { prop_assert!(weights.contains_key(*s)); }
        cleanup(&db);
    }
}
