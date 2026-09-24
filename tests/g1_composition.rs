//! G1 carried-patch composition — integration tests.
//!
//! Reproduces POC-1 through the REAL kernel effect loop (zero model
//! calls) and exercises the fail-closed strict-lineage / baseline /
//! no-members paths. Env-dependent tests are serialized by a mutex
//! (DAK_CODEFIX_WORKSPACE is process-global).

use serde_json::json;
use std::path::PathBuf;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("g1test_{tag}_{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_db(path: &str) -> rusqlite::Connection {
    deterministic_ai_kernel::providers::storage::open_initialized(path).unwrap()
}

fn apply_step(step_id: &str) -> serde_json::Value {
    json!({
        "step_id": step_id,
        "required_capability": "Executor",
        "detail": "apply patch",
        "primitive": {"id": step_id, "kind": "Compute",
            "payload": {"detail": "apply patch", "operation": "none",
                        "requires_llm": false, "step_kind": "ApplyPatch"}},
        "constraints": [{"target":"worker","key":"required_capability","value":"Executor"}],
        "artifact_requirements": [], "inputs": ["patch_v1"],
        "outputs": ["patch_apply_evidence"], "metadata": {"step_kind": "ApplyPatch"}
    })
}
fn run_tests_step() -> serde_json::Value {
    json!({
        "step_id": "04_run_tests",
        "required_capability": "Executor",
        "detail": "run tests",
        "primitive": {"id":"04_run_tests","kind":"Compute",
            "payload":{"detail":"run tests","operation":"none","requires_llm":false,"step_kind":"RunTests"}},
        "constraints":[{"target":"worker","key":"required_capability","value":"Executor"}],
        "artifact_requirements":[],"inputs":[],"outputs":["test_report_v1"],
        "metadata":{"step_kind":"RunTests"}
    })
}
fn validate_step() -> serde_json::Value {
    json!({
        "step_id": "05_validate_patch",
        "required_capability": "Verifier",
        "detail": "validate patch",
        "primitive": {"id":"05_validate_patch","kind":"Route",
            "payload":{"detail":"validate patch","operation":"none","requires_llm":true,"step_kind":"ValidatePatch"}},
        "constraints":[{"target":"worker","key":"required_capability","value":"Verifier"}],
        "artifact_requirements":[],"inputs":["patch_apply_evidence","test_report_v1"],
        "outputs":["route_decision"],"metadata":{"step_kind":"ValidatePatch"}
    })
}

fn comp_spec(members: Vec<String>, baseline_hash: &str, target_file: &str) -> String {
    let spec = json!({
        "version": 1, "spec_id": "g1_test_spec",
        "steps": [apply_step("03_apply_patch"), run_tests_step(), validate_step()],
        "transitions": [],
        "dependencies": [
            {"step_id":"04_run_tests","depends_on":["03_apply_patch"]},
            {"step_id":"05_validate_patch","depends_on":["04_run_tests"]}
        ],
        "policies": [], "artifact_schemas": {},
        "composition": {"members": members, "baseline_hash": baseline_hash, "target_file": target_file}
    });
    spec.to_string()
}

fn insert_member(conn: &rusqlite::Connection, member: &str, patch: &serde_json::Value) {
    conn.execute(
        "INSERT INTO tasks (task_id, task_class, exec_spec) VALUES (?1,'CodeFix','{}')",
        [member],
    )
    .unwrap();
    let payload =
        json!({ "patch_v1": patch, "patch_shape_validation": "ok", "context_occurrences": 1 });
    conn.execute(
        "INSERT INTO semantic_artifacts (task_id, step_id, source_generation, artifact_type, payload) VALUES (?1,'02_patch_code',1,'primitive_result_v1',?2)",
        rusqlite::params![member, payload.to_string()],
    )
    .unwrap();
}

fn insert_composition(conn: &rusqlite::Connection, task: &str, spec_json: &str) {
    conn.execute(
        "INSERT INTO tasks (task_id, task_class, exec_spec) VALUES (?1,'CodeFix',?2)",
        [task, spec_json],
    )
    .unwrap();
    conn.execute("INSERT INTO semantic_bias_artifacts (task_id, input_representation) VALUES (?1,'g1 test composition')", [task])
        .unwrap();
}

fn blake3_hex(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

/// Full POC-1 reproduction through the real effect loop: carried member
/// patch applied on the anchored baseline → real tests pass → completion
/// gate satisfied → task Completed. Zero model calls.
#[test]
fn composition_full_chain_verified_success() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = temp_dir("full");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let baseline = "def f():\n    return 1\n";
    std::fs::write(ws.join("calc.py"), baseline).unwrap();
    std::fs::write(
        ws.join("test_calc.py"),
        "from calc import f\ndef test_f():\n    assert f() == 2\n",
    )
    .unwrap();
    let db_path = root.join("comp.db");
    let db_str = db_path.to_str().unwrap();
    let conn = open_db(db_str);

    let member = "member_task_1";
    let patch = json!({
        "version": "patch_v1",
        "target_file": ws.join("calc.py").to_str().unwrap(),
        "context_before": "    return 1\n",
        "replacement": "    return 2\n",
        "reason": "fix f"
    });
    insert_member(&conn, member, &patch);
    let target = ws.join("calc.py").to_str().unwrap().to_string();
    let spec = comp_spec(vec![member.to_string()], &blake3_hex(baseline), &target);
    insert_composition(&conn, "g1_comp", &spec);
    drop(conn);

    std::env::set_var("DAK_CODEFIX_WORKSPACE", ws.to_str().unwrap());
    let result = deterministic_ai_kernel::effects::execute_effects(db_str, "g1_comp");
    assert!(
        result.is_ok(),
        "composition should complete: {:?}",
        result.err()
    );

    let conn = open_db(db_str);
    let state: String = conn
        .query_row("SELECT status FROM step_status WHERE task_id='g1_comp' AND step_id='05_validate_patch'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "committed");
    let applied = std::fs::read_to_string(ws.join("calc.py")).unwrap();
    assert!(
        applied.contains("return 2"),
        "carried patch must be applied"
    );
    std::env::remove_var("DAK_CODEFIX_WORKSPACE");
}

/// Baseline anchor mismatch must fail closed (never trust unanchored bytes).
#[test]
fn composition_baseline_mismatch_fails_closed() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = temp_dir("baseline_mm");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let baseline = "def f():\n    return 1\n";
    // Write DIFFERENT bytes than the anchored baseline.
    std::fs::write(ws.join("calc.py"), "def f():\n    return 999\n").unwrap();
    let db_path = root.join("comp.db");
    let db_str = db_path.to_str().unwrap();
    let conn = open_db(db_str);
    let member = "member_task_1";
    let patch = json!({
        "version": "patch_v1",
        "target_file": ws.join("calc.py").to_str().unwrap(),
        "context_before": "    return 1\n",
        "replacement": "    return 2\n",
        "reason": "fix f"
    });
    insert_member(&conn, member, &patch);
    let target = ws.join("calc.py").to_str().unwrap().to_string();
    let spec = comp_spec(vec![member.to_string()], &blake3_hex(baseline), &target);
    insert_composition(&conn, "g1_comp", &spec);
    drop(conn);
    std::env::set_var("DAK_CODEFIX_WORKSPACE", ws.to_str().unwrap());
    let result = deterministic_ai_kernel::effects::execute_effects(db_str, "g1_comp");
    assert!(result.is_err(), "baseline mismatch must fail closed");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("baseline mismatch"), "got: {msg}");
    std::env::remove_var("DAK_CODEFIX_WORKSPACE");
}

/// A member that does not exist must fail strict lineage (fail-closed).
#[test]
fn composition_missing_member_fails_lineage() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = temp_dir("missing_member");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let baseline = "def f():\n    return 1\n";
    std::fs::write(ws.join("calc.py"), baseline).unwrap();
    let db_path = root.join("comp.db");
    let db_str = db_path.to_str().unwrap();
    let conn = open_db(db_str);
    // No member inserted — reference a non-existent member.
    let target = ws.join("calc.py").to_str().unwrap().to_string();
    let spec = comp_spec(
        vec!["ghost_member".to_string()],
        &blake3_hex(baseline),
        &target,
    );
    insert_composition(&conn, "g1_comp", &spec);
    drop(conn);
    std::env::set_var("DAK_CODEFIX_WORKSPACE", ws.to_str().unwrap());
    let result = deterministic_ai_kernel::effects::execute_effects(db_str, "g1_comp");
    assert!(result.is_err(), "missing member must fail closed");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("does not exist"), "got: {msg}");
    std::env::remove_var("DAK_CODEFIX_WORKSPACE");
}

/// Zero members must be refused (no fabricated success).
#[test]
fn composition_zero_members_fails_closed() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = temp_dir("zero_members");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let baseline = "def f():\n    return 1\n";
    std::fs::write(ws.join("calc.py"), baseline).unwrap();
    let db_path = root.join("comp.db");
    let db_str = db_path.to_str().unwrap();
    let conn = open_db(db_str);
    let target = ws.join("calc.py").to_str().unwrap().to_string();
    let spec = comp_spec(Vec::<String>::new(), &blake3_hex(baseline), &target);
    insert_composition(&conn, "g1_comp", &spec);
    drop(conn);
    std::env::set_var("DAK_CODEFIX_WORKSPACE", ws.to_str().unwrap());
    let result = deterministic_ai_kernel::effects::execute_effects(db_str, "g1_comp");
    assert!(result.is_err(), "zero members must fail closed");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("no members"), "got: {msg}");
    std::env::remove_var("DAK_CODEFIX_WORKSPACE");
}

/// Multi-member composition applies patches IN ORDER with sequential
/// grounding (member 2 grounded against the state after member 1).
#[test]
fn composition_multi_member_ordered_apply() {
    let _guard = ENV_LOCK.lock().unwrap();
    let root = temp_dir("multi_member");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let baseline = "def a():\n    return 1\n\ndef b():\n    return 10\n";
    std::fs::write(ws.join("calc.py"), baseline).unwrap();
    std::fs::write(
        ws.join("test_calc.py"),
        "from calc import a, b\ndef test_a():\n    assert a() == 2\ndef test_b():\n    assert b() == 20\n",
    )
    .unwrap();
    let db_path = root.join("comp.db");
    let db_str = db_path.to_str().unwrap();
    let conn = open_db(db_str);
    let target = ws.join("calc.py").to_str().unwrap().to_string();
    let m1 = json!({
        "version":"patch_v1","target_file": &target,
        "context_before": "    return 1\n",
        "replacement": "    return 2\n", "reason":"fix a"
    });
    let m2 = json!({
        "version":"patch_v1","target_file": &target,
        "context_before": "    return 10\n",
        "replacement": "    return 20\n", "reason":"fix b"
    });
    insert_member(&conn, "member_a", &m1);
    insert_member(&conn, "member_b", &m2);
    let spec = comp_spec(
        vec!["member_a".to_string(), "member_b".to_string()],
        &blake3_hex(baseline),
        &target,
    );
    insert_composition(&conn, "g1_comp", &spec);
    drop(conn);
    std::env::set_var("DAK_CODEFIX_WORKSPACE", ws.to_str().unwrap());
    let result = deterministic_ai_kernel::effects::execute_effects(db_str, "g1_comp");
    assert!(
        result.is_ok(),
        "multi-member composition should complete: {:?}",
        result.err()
    );
    let applied = std::fs::read_to_string(ws.join("calc.py")).unwrap();
    assert!(applied.contains("return 2"), "member 1 applied");
    assert!(applied.contains("return 20"), "member 2 applied");
    std::env::remove_var("DAK_CODEFIX_WORKSPACE");
}

/// G1 across the merge boundary: run a real composition through the kernel
/// effect loop, then feed the artifact the kernel ACTUALLY persisted into the
/// analyzer's v2 verifier.
///
/// The producer (`src/effects.rs`) and the verifier
/// (`src/analyzer/evidence_chain_v2.rs`) lived on different branches until
/// analyzer and orchestrator-rebuild were merged, and nothing exercised them
/// together: the verifier's own tests used hand-built fixtures, while the
/// kernel never emitted `composed_state_blake3`, so `parse_composition_inputs`
/// refused real kernel output with "field 'composed_state_blake3' missing".
/// This test is what keeps the two halves joined.
#[test]
fn kernel_composition_artifact_verifies_through_analyzer_v2() {
    use deterministic_ai_kernel::analyzer::evidence_chain_v2::{
        parse_composition_inputs, verify_composition_chain, OVERALL_EVIDENCED,
    };

    let _guard = ENV_LOCK.lock().unwrap();
    let root = temp_dir("chain_e2e");
    let ws = root.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let baseline = "def f():\n    return 1\n";
    std::fs::write(ws.join("calc.py"), baseline).unwrap();
    std::fs::write(
        ws.join("test_calc.py"),
        "from calc import f\ndef test_f():\n    assert f() == 2\n",
    )
    .unwrap();
    let db_path = root.join("chain.db");
    let db_str = db_path.to_str().unwrap();
    let conn = open_db(db_str);

    let member = "chain_member_1";
    let patch = json!({
        "version": "patch_v1",
        "target_file": ws.join("calc.py").to_str().unwrap(),
        "context_before": "    return 1\n",
        "replacement": "    return 2\n",
        "reason": "fix f"
    });
    insert_member(&conn, member, &patch);
    let target = ws.join("calc.py").to_str().unwrap().to_string();
    let spec = comp_spec(vec![member.to_string()], &blake3_hex(baseline), &target);
    insert_composition(&conn, "g1_chain", &spec);
    drop(conn);

    std::env::set_var("DAK_CODEFIX_WORKSPACE", ws.to_str().unwrap());
    deterministic_ai_kernel::effects::execute_effects(db_str, "g1_chain")
        .expect("composition should complete");

    // Read back exactly what the kernel persisted — no hand-built fixture.
    let conn = open_db(db_str);
    let artifact: String = conn
        .query_row(
            "SELECT payload FROM semantic_artifacts \
             WHERE task_id='g1_chain' AND step_id='03_apply_patch' \
             ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("kernel must persist the combined apply-evidence artifact");

    let inputs = parse_composition_inputs(&artifact)
        .unwrap_or_else(|e| panic!("v2 verifier must accept real kernel output, got: {e}"));

    assert_eq!(inputs.target_file, target);
    assert_eq!(inputs.baseline_hash, blake3_hex(baseline));
    assert_eq!(inputs.members.len(), 1);
    assert_eq!(inputs.members[0].member_task_id, member);
    assert!(inputs.members[0].applied);

    // composed_state_blake3 is measured from the bytes on disk rather than
    // copied from the member's self-report, so it must equal the real content
    // AND agree with what apply_patch claimed.
    let applied = std::fs::read_to_string(ws.join("calc.py")).unwrap();
    assert_eq!(inputs.composed_state_blake3, blake3_hex(&applied));
    assert_eq!(
        inputs.composed_state_blake3, inputs.members[0].post_image_blake3,
        "measured composed state must agree with the member's reported post image"
    );

    // The kernel persists the report wrapped as {"test_report_v1": {...}};
    // the verifier validates the inner document, which is what
    // `analyzer_chain_verify --composition --test-report <file>` is handed.
    let wrapped: String = conn
        .query_row(
            "SELECT payload FROM semantic_artifacts \
             WHERE task_id='g1_chain' AND step_id='04_run_tests' \
             ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("kernel must persist a test report artifact");
    let wrapped: serde_json::Value = serde_json::from_str(&wrapped).unwrap();
    let report_bytes = serde_json::to_string(&wrapped["test_report_v1"]).unwrap();

    let core = verify_composition_chain(&inputs, Some(&report_bytes));
    let links: Vec<String> = core
        .links
        .iter()
        .map(|l| format!("{}={}", l.name, l.status))
        .collect();
    assert_eq!(
        core.overall, OVERALL_EVIDENCED,
        "chain must verify end-to-end; links: {links:?}"
    );
    assert!(
        core.links.iter().all(|l| l.status == "verified"),
        "every link must verify positively, got: {links:?}"
    );

    std::env::remove_var("DAK_CODEFIX_WORKSPACE");
}
