//! 4H §6 — ROUTER / PLANNER FORENSICS
//!
//! Adversarial corpus through the PRODUCTION routing path (`build_plan`,
//! the exact function pipeline-run uses). No model: this verifies
//! parser + routing determinism only.
//!
//! Invariants:
//!  A. analytical questions (EN/RU) → AnswerQuestion, never ExecuteChanges
//!  B. CodeFix payloads → canonical CodeFix step kinds
//!  C. repository-action imperatives → ExecuteChanges
//!  D. negative constraints honored ("do not modify", "не выполняй изменений",
//!     "only explain") → AnswerQuestion even with action verbs present
//!  E. no character-level shredding: every fragment is a meaningful chunk

use deterministic_ai_kernel::planner_pipeline::build_plan;

fn route_kind(payload: &str) -> String {
    let report = build_plan(payload, 42).expect("production build_plan must not fail");
    report.plan.spec.steps[0]
        .primitive
        .as_ref()
        .and_then(|p| p.payload.get("step_kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string()
}

fn fragments(payload: &str) -> Vec<String> {
    let report = build_plan(payload, 42).expect("build_plan");
    report.plan.steps.clone()
}

// ── A. analytical → AnswerQuestion ─────────────────────────────────────

#[test]
fn forensic_en_analytical_routes_to_answer_question() {
    let cases = [
        "What is the difference between a process and a thread?",
        "Explain how garbage collection works in managed runtimes.",
        "Compare merge sort and heap sort in terms of memory usage.",
        "How many primes are there between 1 and 50?",
        "If all cats are mammals and some mammals swim, do all cats swim?",
        "Why does TCP need a three-way handshake?",
        "A train leaves at 9:15 and arrives at 11:45. How long is the trip?",
    ];
    for p in cases {
        assert_eq!(route_kind(p), "AnswerQuestion", "misrouted: {p}");
    }
}

#[test]
fn forensic_ru_analytical_routes_to_answer_question() {
    let cases = [
        "Объясни, чем поток отличается от процесса.",
        "Сравни mergesort и heapsort по памяти.",
        "Почему TCP требует трёхэтапное рукопожатие?",
        "Сколько простых чисел между 1 и 50?",
        "Проанализируй, почему конвейер может простаивать при одном воркере.",
        "Опиши, как устроен event loop в асинхронных рантаймах.",
        "Если все кошки млекопитающие, а некоторые млекопитающие плавают — все ли кошки плавают?",
    ];
    for p in cases {
        assert_eq!(route_kind(p), "AnswerQuestion", "misrouted: {p}");
    }
}

#[test]
fn forensic_technical_prose_with_conjunctions_stays_analytical() {
    // Prose containing conjunctions must not be shredded into execute
    // steps nor routed to ExecuteChanges.
    let cases = [
        "Explain how the scheduler works and why it needs priorities and what happens when a lease expires.",
        "Расскажи как работает сборщик мусора и почему нужны поколения и когда случаются паузы.",
        "Describe the request lifecycle and identify where timeouts apply and explain retry policy.",
    ];
    for p in cases {
        assert_eq!(route_kind(p), "AnswerQuestion", "misrouted: {p}");
    }
}

// ── B. CodeFix payloads → canonical CodeFix flow ───────────────────────

#[test]
fn forensic_codefix_payload_routes_to_codefix_chain() {
    let p = "Step 1 read repository /tmp/ws/calc.py
Step 2 find bug
Step 3 patch code
Step 4 apply patch
Step 5 run tests
Step 6 validate patch
Fix the multiply function.";
    let report = build_plan(p, 42).expect("build_plan");
    let kinds: Vec<String> = report
        .plan
        .spec
        .steps
        .iter()
        .map(|s| {
            s.primitive
                .as_ref()
                .and_then(|p| p.payload.get("step_kind"))
                .and_then(|v| v.as_str())
                .unwrap_or("none")
                .to_string()
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "ReadRepository",
            "LocateBug",
            "PatchCode",
            "ApplyPatch",
            "RunTests",
            "ValidatePatch"
        ],
        "canonical CodeFix chain broken"
    );
}

// ── C. repository-action imperatives → ExecuteChanges ──────────────────

#[test]
fn forensic_imperatives_route_to_execute_changes() {
    let cases = [
        "Refactor the scheduler module to split lease handling.",
        "Добавь тесты для модуля парсера и обнови документацию.",
        "Создай новый модуль для метрик и подключи его в lib.",
    ];
    for p in cases {
        assert_eq!(route_kind(p), "ExecuteChanges", "must stay imperative: {p}");
    }
}

// ── D. negative constraints honored ────────────────────────────────────

#[test]
fn forensic_negative_constraints_force_answer_question() {
    let cases = [
        "Analyze the parser and do not modify any files.",
        "Explain the routing logic but do not execute any commands.",
        "Проанализируй код и не выполняй никаких изменений файлов.",
        "Объясни архитектуру, только объясни — ничего не меняй.",
        "Review the scheduler design; only explain, do not change anything.",
        "Не меняя файлы, объясни, почему тесты могут падать.",
    ];
    for p in cases {
        assert_eq!(
            route_kind(p),
            "AnswerQuestion",
            "negative constraint ignored: {p}"
        );
    }
}

// ── E. no character-level shredding ────────────────────────────────────

#[test]
fn forensic_no_character_shredding() {
    // Conjunction letters inside words must never become split points.
    let cases = [
        "Исследовать проблему и подготовить отчёт.",
        "Initialize the iterator and iterate over items.",
        "Подготовить сводку по инцидентам и отправить.",
    ];
    for p in cases {
        for frag in fragments(p) {
            assert!(
                frag.chars().count() >= 3,
                "fragment too small (shredding): '{frag}' from '{p}'"
            );
            assert!(
                !matches!(frag.trim(), "и" | "и," | "a" | "and"),
                "single conjunction became a fragment: '{frag}' from '{p}'"
            );
        }
    }
}
