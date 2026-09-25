//! Layer-2 POC — verifier-driven feedback loop integration tests.
//!
//! LAYER2_VERIFIER_FEEDBACK_SPEC §5 + security review conditions C1/C3:
//! the loop re-enters the canonical executor path; failing test names are
//! advisory prompt content only; exhaustion is an honest failure.
//!
//! All tests share one process-global mock LLM (mode-switched) and one
//! mutex, because the provider registry and DAK_FEEDBACK_LOOP are
//! process-global.

use deterministic_ai_kernel::effects::execute_effects;
use deterministic_ai_kernel::event_bus::EventBus;
use deterministic_ai_kernel::execution::feedback;
use deterministic_ai_kernel::providers::{register_llm, LlmProvider, LlmResponse};
use deterministic_ai_kernel::workflow::contract::{steps_to_exec_spec, Step, StepKind};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);
static TEST_LOCK: Mutex<()> = Mutex::new(());
static REGISTER: Once = Once::new();

// Mode: "convert" = wrong patch first, correct patch when the prompt
// carries the feedback block; "always_wrong" = never fixes (but varies
// the patch so identical-patch detection does not fire);
// "identical" = the exact same patch on every call.
static MODE: Mutex<&'static str> = Mutex::new("convert");
static CALLS: AtomicU64 = AtomicU64::new(0);

struct LoopMockLlm;

impl LlmProvider for LoopMockLlm {
    fn coding_assistant(&self, prompt: &str) -> anyhow::Result<String> {
        Ok(self.execute_llm(prompt, None)?.text)
    }

    fn execute_llm(
        &self,
        prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<LlmResponse> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        let target = prompt
            .lines()
            .find_map(|l| l.strip_prefix("FILE: "))
            .unwrap_or("")
            .to_string();
        let mode = *MODE.lock().unwrap_or_else(|e| e.into_inner());
        let replacement = match mode {
            "convert" if prompt.contains("VERIFICATION FEEDBACK") => "    return a * b",
            "convert" => "    return a - b",
            // vary the patch so the identical-patch stop does not fire
            "always_wrong" if prompt.contains("VERIFICATION FEEDBACK") => {
                "    return a - b  # attempt 2"
            }
            _ => "    return a - b", // identical
        };
        let text = json!({
            "version": "patch_v1",
            "target_file": target,
            "context_before": "    return a + b",
            "replacement": replacement,
            "reason": "loop mock"
        })
        .to_string();
        Ok(LlmResponse {
            text,
            model_name: "loop-mock".to_string(),
            model_version: None,
        })
    }

    fn embed_text(&self, _prompt: &str) -> anyhow::Result<Vec<f32>> {
        Ok(vec![0.0])
    }
}

fn ensure_mock() {
    REGISTER.call_once(|| {
        register_llm(Box::new(LoopMockLlm));
    });
}

fn unique(name: &str) -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{name}_{n}_{nanos}")
}

fn fresh_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(unique(name));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn python_project(dir: &std::path::Path) {
    std::fs::write(dir.join("calc.py"), "def multiply(a, b):\n    return a + b\n").unwrap();
    std::fs::write(
        dir.join("test_calc.py"),
        "from calc import multiply\n\n\ndef test_multiply():\n    assert multiply(2, 3) == 6\n",
    )
    .unwrap();
}

/// Full CodeFix chain: read -> patch -> apply -> run -> validate, with the
/// workspace confined on every primitive.
fn loop_chain_spec(workspace: &str) -> serde_json::Value {
    let mut spec = steps_to_exec_spec(&[
        Step {
            kind: StepKind::ReadRepository,
            detail: Some("read repository".to_string()),
        },
        Step {
            kind: StepKind::PatchCode,
            detail: Some("patch code".to_string()),
        },
        Step {
            kind: StepKind::ApplyPatch,
            detail: Some("apply patch".to_string()),
        },
        Step {
            kind: StepKind::RunTests,
            detail: Some("run tests".to_string()),
        },
        Step {
            kind: StepKind::ValidatePatch,
            detail: Some("validate patch".to_string()),
        },
    ]);
    for step in spec.steps.iter_mut() {
        if let Some(prim) = step.primitive.as_mut() {
            prim.payload
                .as_object_mut()
                .unwrap()
                .insert("workspace".to_string(), json!(workspace));
        }
    }
    serde_json::to_value(&spec).unwrap()
}

fn create_task(task_id: &str, spec_json: &str, payload: &str) -> String {
    let db = std::env::temp_dir().join(format!("{}.db", unique("dak_loop")));
    let db_str = db.to_string_lossy().into_owned();
    let conn = deterministic_ai_kernel::providers::storage::open_initialized(&db_str).unwrap();
    conn.execute(
        "INSERT INTO tasks (task_id, task_class, exec_spec) VALUES (?1, 'CodeFix', ?2)",
        rusqlite::params![task_id, spec_json],
    )
    .unwrap();
    drop(conn);
    std::fs::create_dir_all("artifacts").unwrap();
    std::fs::write(format!("artifacts/pipeline_input.{task_id}.txt"), payload).unwrap();
    db_str
}

fn cleanup(task_id: &str, db_str: &str, ws: &std::path::Path) {
    let _ = std::fs::remove_file(format!("artifacts/pipeline_input.{task_id}.txt"));
    let _ = std::fs::remove_file(db_str);
    let _ = std::fs::remove_file(format!("{db_str}-wal"));
    let _ = std::fs::remove_file(format!("{db_str}-shm"));
    let _ = std::fs::remove_dir_all(ws);
}

fn event_types(db: &str, task_id: &str) -> Vec<String> {
    EventBus::new(db)
        .unwrap()
        .query(task_id)
        .unwrap()
        .iter()
        .map(|e| e.event_type.clone())
        .collect()
}

#[test]
fn feedback_loop_converts_failing_task() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_mock();
    (*MODE.lock().unwrap_or_else(|e| e.into_inner())) = "convert";
    CALLS.store(0, Ordering::SeqCst);
    std::env::remove_var("DAK_FEEDBACK_LOOP");

    let ws = fresh_dir("loop_convert");
    python_project(&ws);
    let calc = ws.join("calc.py");
    let task_id = unique("loop-convert");
    let payload = format!(
        "Step 1 read repository {0}\nStep 2 find bug\nStep 3 patch code\nStep 4 apply patch\nStep 5 run tests\nStep 6 validate patch\nFix multiply: returns a + b instead of a * b.",
        calc.display()
    );
    let db = create_task(&task_id, &loop_chain_spec(ws.to_str().unwrap()).to_string(), &payload);

    execute_effects(&db, &task_id).expect("loop must convert and the chain must complete");

    let events = event_types(&db, &task_id);
    assert!(events.contains(&feedback::FEEDBACK_CYCLE_STARTED.to_string()), "{events:?}");
    assert!(events.contains(&feedback::FEEDBACK_ATTEMPT.to_string()), "{events:?}");
    assert!(events.contains(&feedback::FEEDBACK_CONVERTED.to_string()), "{events:?}");
    assert!(!events.contains(&feedback::FEEDBACK_EXHAUSTED.to_string()));

    // converted on the first feedback attempt => 2 patch calls total
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);

    // the final file carries the feedback-produced fix
    let content = std::fs::read_to_string(&calc).unwrap();
    assert!(content.contains("return a * b"), "{content}");

    cleanup(&task_id, &db, &ws);
}

#[test]
fn feedback_loop_exhausts_budget_honestly() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_mock();
    (*MODE.lock().unwrap_or_else(|e| e.into_inner())) = "always_wrong";
    CALLS.store(0, Ordering::SeqCst);
    std::env::remove_var("DAK_FEEDBACK_LOOP");

    let ws = fresh_dir("loop_exhaust");
    python_project(&ws);
    let calc = ws.join("calc.py");
    let task_id = unique("loop-exhaust");
    let payload = format!(
        "Step 1 read repository {0}\nStep 2 find bug\nStep 3 patch code\nStep 4 apply patch\nStep 5 run tests\nStep 6 validate patch\nFix multiply: returns a + b instead of a * b.",
        calc.display()
    );
    let db = create_task(&task_id, &loop_chain_spec(ws.to_str().unwrap()).to_string(), &payload);

    let err = execute_effects(&db, &task_id).expect_err("must fail honestly");
    assert!(err.to_string().contains("real tests failed"), "{err}");

    let events = event_types(&db, &task_id);
    assert!(events.contains(&feedback::FEEDBACK_CYCLE_STARTED.to_string()));
    assert!(events.contains(&feedback::FEEDBACK_EXHAUSTED.to_string()));
    assert!(!events.contains(&feedback::FEEDBACK_CONVERTED.to_string()));
    // budget = 2 feedback attempts => 3 patch calls total
    assert_eq!(CALLS.load(Ordering::SeqCst), 1 + feedback::MAX_FEEDBACK_ATTEMPTS as u64);

    cleanup(&task_id, &db, &ws);
}

#[test]
fn feedback_loop_stops_on_identical_patch() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_mock();
    (*MODE.lock().unwrap_or_else(|e| e.into_inner())) = "identical";
    CALLS.store(0, Ordering::SeqCst);
    std::env::remove_var("DAK_FEEDBACK_LOOP");

    let ws = fresh_dir("loop_identical");
    python_project(&ws);
    let calc = ws.join("calc.py");
    let task_id = unique("loop-identical");
    let payload = format!(
        "Step 1 read repository {0}\nStep 2 find bug\nStep 3 patch code\nStep 4 apply patch\nStep 5 run tests\nStep 6 validate patch\nFix multiply: returns a + b instead of a * b.",
        calc.display()
    );
    let db = create_task(&task_id, &loop_chain_spec(ws.to_str().unwrap()).to_string(), &payload);

    let err = execute_effects(&db, &task_id).expect_err("must fail honestly");
    assert!(err.to_string().contains("real tests failed"), "{err}");

    // identical patch on attempt 1 => stop immediately (2 patch calls total)
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);

    let events = event_types(&db, &task_id);
    let bus = EventBus::new(&db).unwrap();
    let exhausted = bus.query(&task_id).unwrap();
    let ex = exhausted
        .iter()
        .find(|e| e.event_type == feedback::FEEDBACK_EXHAUSTED)
        .expect("EXHAUSTED event");
    assert!(ex.payload.contains("identical_patch"), "{}", ex.payload);
    let _ = events;

    cleanup(&task_id, &db, &ws);
}

#[test]
fn feedback_loop_disabled_by_env() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_mock();
    (*MODE.lock().unwrap_or_else(|e| e.into_inner())) = "convert";
    CALLS.store(0, Ordering::SeqCst);
    std::env::set_var("DAK_FEEDBACK_LOOP", "off");

    let ws = fresh_dir("loop_off");
    python_project(&ws);
    let calc = ws.join("calc.py");
    let task_id = unique("loop-off");
    let payload = format!(
        "Step 1 read repository {0}\nStep 2 find bug\nStep 3 patch code\nStep 4 apply patch\nStep 5 run tests\nStep 6 validate patch\nFix multiply: returns a + b instead of a * b.",
        calc.display()
    );
    let db = create_task(&task_id, &loop_chain_spec(ws.to_str().unwrap()).to_string(), &payload);

    let err = execute_effects(&db, &task_id).expect_err("disabled loop keeps terminal failure");
    assert!(err.to_string().contains("real tests failed"), "{err}");
    // no loop: exactly one patch call, no feedback events
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    let events = event_types(&db, &task_id);
    assert!(!events.contains(&feedback::FEEDBACK_CYCLE_STARTED.to_string()));

    std::env::remove_var("DAK_FEEDBACK_LOOP");
    cleanup(&task_id, &db, &ws);
}
