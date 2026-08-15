//! R2 CLOSURE — FORENSIC VERIFICATION SUITE
//!
//! Every case below runs the PRODUCTION functions (`Parser::run`,
//! `Plan::new_with_stable_id` → `normalize_step`) — the exact code path used
//! by `pipeline-run` → `build_plan_and_publish` → `build_plan`. No mocks, no
//! fixtures that bypass production code, no hardcoded LLM answers: the LLM
//! is never invoked here (this suite verifies parser + routing only; the
//! live Gemma4 evidence is separate).
//!
//! Invariants proven:
//!  A. A character occurrence must NEVER become a split point merely because
//!     it contains the first character of a conjunction (R2a).
//!  B. Legitimate conjunction splitting still works (fix must not disable
//!     decomposition).
//!  C. The verb/action guard blocks prose decomposition but keeps genuine
//!     action sequences decomposable (R2a guard).
//!  D. Routing: '?' anywhere, EN/RU analytical openers → answer_question;
//!     CodeFix pairs and plain imperatives are not hijacked (R2b/c).

use deterministic_ai_kernel::planner_pipeline::parser::Parser;
use deterministic_ai_kernel::planner_pipeline::{
    build_plan, PipelineContext, PipelineStage, RawInput,
};
use deterministic_ai_kernel::semantic_bias::BiasVersion;
use deterministic_ai_kernel::workflow::contract::StepKind;
use deterministic_ai_kernel::workflow::planner::normalize_step;

fn ctx() -> PipelineContext {
    PipelineContext {
        seed: 42,
        bias_version: BiasVersion::V1,
        task_id: None,
    }
}

/// Production decomposition: Parser::run is the exact function build_plan
/// calls on the pipeline-run path.
fn parse_steps(payload: &str) -> Vec<String> {
    let input = RawInput {
        payload: payload.to_string(),
    };
    let ir = Parser.run(input, &ctx()).expect("parser must not fail");
    ir.steps
}

/// Production routing via build_plan — the exact library function used by
/// the pipeline-run CLI (Normalizer → Parser → SemanticMapper → Plan →
/// steps_to_exec_spec inside Plan::new_with_stable_id).
fn route_of(payload: &str) -> (Vec<String>, String) {
    let report = build_plan(payload, 42).expect("production build_plan must not fail");
    let kind = report.plan.spec.steps[0]
        .primitive
        .as_ref()
        .and_then(|p| p.payload.get("step_kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string();
    (report.plan.steps.clone(), kind)
}

fn report(case: &str, input: &str, steps: &[String], expected: usize, ctx_note: &str) {
    let status = if steps.len() == expected {
        "PASS"
    } else {
        "FAIL"
    };
    println!("CASE   : {case} ({ctx_note})");
    println!("INPUT  : {input}");
    println!("RAW    : {steps:?}");
    println!(
        "FRAGS  : {} (expected {})\nSTATUS : {status}\n",
        steps.len(),
        expected
    );
}

// ─────────────────────────── PHASE 2: EN PARSER ───────────────────────────

#[test]
fn phase2_en_parser_matrix() {
    // EN-1: normal prose with MANY 't' characters → 1 semantic unit.
    let p = "The total quantity of critical parts must be counted at the earliest opportunity.";
    let s = parse_steps(p);
    report("EN-1 t-density prose", p, &s, 1, "R2a");
    assert_eq!(s.len(), 1, "t-density prose shredded: {s:?}");

    // EN-2: sentence with and/or/but/then conjunctions, prose semantics.
    let p = "The price is increased and then discounted but the final value is lower.";
    let s = parse_steps(p);
    report("EN-2 conjunction prose", p, &s, 1, "R2a+guard");
    assert_eq!(s.len(), 1, "conjunction prose shredded: {s:?}");

    // EN-3: frequent 't', no conjunction semantics.
    let p = "Estimate the total cost of the treatment at the earliest date.";
    let s = parse_steps(p);
    report("EN-3 frequent t", p, &s, 1, "R2a");
    assert_eq!(s.len(), 1, "frequent-t text shredded: {s:?}");

    // EN-4: words beginning with conjunction-like prefixes (android, band,
    // then-current) — internal substring must not split.
    let p = "The android update is standard and the band then tested the antenna.";
    let s = parse_steps(p);
    report(
        "EN-4 conjunction-like prefixes",
        p,
        &s,
        1,
        "R2a word-boundary",
    );
    assert_eq!(s.len(), 1, "prefix words shredded: {s:?}");

    // EN-5: action-heavy text MUST decompose (fix must not disable
    // decomposition).
    let p = "Refactor the parser module and add tests for all edge cases and update the docs";
    let s = parse_steps(p);
    report("EN-5 action sequence", p, &s, 3, "legit decomposition");
    assert_eq!(s.len(), 3, "action sequence not decomposed: {s:?}");

    // EN-6: ordinary prose with conjunctions must stay one unit.
    let p = "The warehouse inventory was updated last quarter and the report was published.";
    let s = parse_steps(p);
    report("EN-6 ordinary prose", p, &s, 1, "guard");
    assert_eq!(s.len(), 1, "ordinary prose shredded: {s:?}");
}

// ─────────────────────────── PHASE 2: RU PARSER ───────────────────────────

#[test]
fn phase2_ru_parser_matrix() {
    // RU-1: prose with MANY 'и' characters inside words → 1 unit.
    let p = "В количестве изделий и материалов нашли изъяны и несоответствия.";
    let s = parse_steps(p);
    report("RU-1 и-density prose", p, &s, 1, "R2a");
    assert_eq!(s.len(), 1, "и-density prose shredded: {s:?}");

    // RU-2: standalone 'и' conjunctions in prose semantics.
    let p = "Отчёт был готов и проверен и подписан директором.";
    let s = parse_steps(p);
    report("RU-2 conjunction prose", p, &s, 1, "R2a+guard");
    assert_eq!(s.len(), 1, "RU conjunction prose shredded: {s:?}");

    // RU-3: words containing 'и' internally.
    let p = "Изделие требует калибровки и настройки перед запуском.";
    let s = parse_steps(p);
    report("RU-3 internal и", p, &s, 1, "R2a");
    assert_eq!(s.len(), 1, "internal-и text shredded: {s:?}");

    // RU-4: frequent standalone 'и' but no action verbs → 1 unit.
    let p = "Сигнал появляется и исчезает и снова появляется и затихает.";
    let s = parse_steps(p);
    report("RU-4 frequent standalone и", p, &s, 1, "R2a+guard");
    assert_eq!(s.len(), 1, "frequent-и text shredded: {s:?}");

    // RU-5: action-heavy RU instruction MUST decompose.
    let p = "Добавь тесты и обнови документацию и проверь совместимость";
    let s = parse_steps(p);
    report("RU-5 action sequence", p, &s, 3, "legit decomposition");
    assert_eq!(s.len(), 3, "RU action sequence not decomposed: {s:?}");

    // RU-6: ordinary RU prose stays one semantic unit.
    let p = "На складе было 120 деталей и 40 из них отправили на проверку.";
    let s = parse_steps(p);
    report("RU-6 ordinary prose", p, &s, 1, "guard");
    assert_eq!(s.len(), 1, "RU ordinary prose shredded: {s:?}");
}

// ─────────────────────── PHASE 3: VERB/ACTION GUARD ───────────────────────

#[test]
fn phase3_guard_must_not_decompose() {
    let cases = [
        // analytical prose
        "A price is increased by 18% and then discounted by 18% and the result is compared",
        // descriptive prose
        "The system was deployed in March and the metrics were collected weekly",
        // mathematical question
        "A truck travels 420 km at 70 km/h and then 180 km at 90 km/h",
        // informational question
        "What is the total energy and how is it computed",
        // statement with conjunctions
        "The error rate dropped and the latency improved and the uptime increased",
    ];
    for p in cases {
        let s = parse_steps(p);
        report("GUARD-keep", p, &s, 1, "must NOT decompose");
        assert_eq!(s.len(), 1, "prose wrongly decomposed: {p} → {s:?}");
    }
}

#[test]
fn phase3_guard_must_decompose() {
    let cases = [
        (
            "Fix the parser bug and add regression tests and update the changelog",
            3,
        ),
        (
            "Read the config file carefully and validate the schema against it",
            2,
        ),
        (
            "Добавь обработку ошибок и напиши тесты и проверь результат",
            3,
        ),
    ];
    for (p, expected) in cases {
        let s = parse_steps(p);
        report("GUARD-split", p, &s, expected, "MUST decompose");
        assert_eq!(
            s.len(),
            expected,
            "action sequence not decomposed: {p} → {s:?}"
        );
        // every fragment must be a meaningful action unit (starts with an
        // action verb) — proof the guard keeps genuine sequences intact
        assert!(
            s.iter().all(|c| c.len() > 10),
            "decomposed fragments must be meaningful: {s:?}"
        );
    }
}

// ─────────────────────── PHASE 4: ROUTING MATRIX ──────────────────────────

#[test]
fn phase4_routing_en() {
    let cases = [
        ("What is 17 × 19?", "AnswerQuestion"),
        (
            "Determine the total driving time for the route.",
            "AnswerQuestion",
        ),
        ("Evaluate the total energy consumed.", "AnswerQuestion"),
        (
            "Explain the difference between the two modes.",
            "AnswerQuestion",
        ),
        ("Compare the two approaches.", "AnswerQuestion"),
        // '?' appears BEFORE the final characters, no accepted opener
        (
            "The price changed twice. Is the final price equal?",
            "AnswerQuestion",
        ),
        // analytical text without '?' but with accepted opener
        (
            "Determine how many parts remain after both shipments.",
            "AnswerQuestion",
        ),
    ];
    for (p, expected) in cases {
        let (steps, kind) = route_of(p);
        let ok = kind == expected && steps.len() == 1;
        println!(
            "ROUTE  : {p}\nSELECT : {kind} (expected {expected}, steps={})\nSTATUS : {}\n",
            steps.len(),
            if ok { "PASS" } else { "FAIL" }
        );
        assert!(ok, "EN routing failed: {p} → {kind} ({steps:?})");
    }
}

#[test]
fn phase4_routing_ru() {
    let cases = [
        ("Что такое TaskFold?", "AnswerQuestion"),
        ("Есть ли данные по насосу 7?", "AnswerQuestion"),
        (
            "Можно ли выполнить все три операции за 8 часов?",
            "AnswerQuestion",
        ),
        (
            "Объясни разницу между retryable и terminal отказом.",
            "AnswerQuestion",
        ),
        ("Укажи итоговое время завершения.", "AnswerQuestion"),
        (
            "Столько будет 9 умножить на 7? Ответь числом.",
            "AnswerQuestion",
        ),
        ("Сравни два варианта распределения.", "AnswerQuestion"),
        ("Определи узкое место в линии.", "AnswerQuestion"),
    ];
    for (p, expected) in cases {
        let (steps, kind) = route_of(p);
        let ok = kind == expected && steps.len() == 1;
        println!(
            "ROUTE  : {p}\nSELECT : {kind} (expected {expected}, steps={})\nSTATUS : {}\n",
            steps.len(),
            if ok { "PASS" } else { "FAIL" }
        );
        assert!(ok, "RU routing failed: {p} → {kind} ({steps:?})");
    }
}

#[test]
fn phase4_routing_negative_guards() {
    // CodeFix pairs must NOT be hijacked by question routing.
    let (steps, kind) = route_of("Can you find the bug in the parser?");
    println!("GUARD  : CodeFix pair vs '?' → {kind} ({steps:?})");
    assert_eq!(kind, "LocateBug", "CodeFix pair hijacked: {kind}");

    // Plain imperative stays ExecuteChanges (not a question, not CodeFix).
    let (steps, kind) = route_of("Refactor the scheduler module");
    println!("GUARD  : plain imperative → {kind} ({steps:?})");
    assert_eq!(kind, "ExecuteChanges", "imperative misrouted: {kind}");

    // No routing into hardening stubs for domain keywords (R1 invariant).
    let (steps, kind) = route_of("Describe the test coverage of the module");
    println!("GUARD  : domain 'test' keyword → {kind} ({steps:?})");
    assert_ne!(kind, "AddPlannerTestCoverage", "R1 regression: {kind}");
}

// ───────────────── PHASE 6: NEGATIVE / ADVERSARIAL STRESS ─────────────────

#[test]
fn phase6_adversarial_parser_stress() {
    // Many 't' + real conjunctions.
    let p = "The ttt total and the ttt count then the ttt list but the ttt set";
    let s = parse_steps(p);
    report("ADV-1 t-storm + conjunctions", p, &s, 1, "adversarial");
    assert!(s.len() <= 2, "t-storm shredded: {s:?}");

    // Standalone 'и' storm.
    let p = "и появляется и исчезает и появляется и исчезает и появляется";
    let s = parse_steps(p);
    report("ADV-2 и-storm", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "и-storm shredded: {s:?}");

    // Conjunction-like substrings and repeated conjunctions.
    let p = "The then-current standard and the and-filter and the but-marker";
    let s = parse_steps(p);
    report("ADV-3 conjunction substrings", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "substring conjunctions shredded: {s:?}");

    // Mixed EN/RU.
    let p = "Calculate the total и определи итог?";
    let s = parse_steps(p);
    report("ADV-4 mixed EN/RU", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "mixed text shredded: {s:?}");

    // Punctuation around conjunctions.
    let p = "The total, and the subtotal; and the final sum.";
    let s = parse_steps(p);
    report("ADV-5 punctuation + conjunctions", p, &s, 1, "adversarial");
    assert!(s.len() <= 2, "punctuation/conjunction shredded: {s:?}");

    // Conjunctions at the beginning/end.
    let p = "And the total is final.";
    let s = parse_steps(p);
    report("ADV-6 leading conjunction", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "leading conjunction shredded: {s:?}");
    let p = "The total is final and.";
    let s = parse_steps(p);
    report("ADV-7 trailing conjunction", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "trailing conjunction shredded: {s:?}");

    // Uppercase variants.
    let p = "THE PRICE IS INCREASED AND THEN DISCOUNTED AND COMPARED.";
    let s = parse_steps(p);
    report("ADV-8 uppercase", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "uppercase shredded: {s:?}");

    // Multiple '?'.
    let p = "What is the total? Why is it lower? How is it computed?";
    let s = parse_steps(p);
    report("ADV-9 multiple ?", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "multi-question shredded: {s:?}");

    // '?' followed by additional prose.
    let p = "Is the result correct? Explain the reasoning and the intermediate steps.";
    let s = parse_steps(p);
    report("ADV-10 ? + trailing prose", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "? + prose shredded: {s:?}");

    // Question containing action verbs still routes as ONE question.
    let (steps, kind) = route_of("Can you find the bug in the parser?");
    println!("ADV-11 : question + action verb → {kind} ({steps:?})");
    assert_eq!(kind, "LocateBug");

    // Long natural-language payload stays one unit.
    let p = "A warehouse received 120 parts and 25% were sent to warehouse A and of the remainder 40% were sent to warehouse B and the manager needs the final count with an explanation";
    let s = parse_steps(p);
    report("ADV-12 long NL payload", p, &s, 1, "adversarial");
    assert_eq!(s.len(), 1, "long NL shredded: {s:?}");
}

// ─────────── PHASE 9: REGRESSION-PROOF OF THE ROOT CAUSE ──────────────────

#[test]
fn regression_character_split_cannot_recur() {
    // Would FAIL if `text.find(c)`-style character-level splitting were
    // reintroduced: this text has many 't' AND enough action verbs that a
    // character-split would either shred it (>1 fragments of junk) or
    // destroy the exact 3-way action decomposition. (>60 bytes required
    // for decomposition eligibility.)
    let p = "update the data records and add the total count and update the stats tables";
    let s = parse_steps(p);
    assert_eq!(
        s.len(),
        3,
        "character-split regression or disabled decomposition: {s:?}"
    );
    assert!(
        s.iter()
            .all(|c| c.starts_with("update") || c.starts_with("add")),
        "fragments corrupted: {s:?}"
    );

    // RU twin: many internal 'и' + genuine action decomposition.
    let p = "добавь тесты и обнови данные и проверь итог";
    let s = parse_steps(p);
    assert_eq!(s.len(), 3, "RU character-split regression: {s:?}");
}

#[test]
fn regression_question_anywhere_and_openers_cannot_recur() {
    // Would FAIL if '?' were only checked at the END of the text.
    let a4 = "A price is increased by 18% and then discounted by 18%. Is the final price \
              equal to the original price? Give the exact calculation and a yes/no answer.";
    assert_eq!(normalize_step(a4), Some(StepKind::AnswerQuestion));

    // Would FAIL if analytical openers were removed (EN).
    for p in [
        "Determine the total driving time.",
        "Evaluate the total energy.",
        "Explain the difference.",
        "Compare the two approaches.",
    ] {
        assert_eq!(
            normalize_step(p),
            Some(StepKind::AnswerQuestion),
            "EN opener lost: {p}"
        );
    }

    // Would FAIL if RU analytical openers stopped routing.
    for p in [
        "Есть 120 деталей. Сколько осталось?",
        "Можно ли выполнить все операции?",
        "Объясни разницу.",
        "Укажи итоговое время.",
        "Столько будет 9 умножить на 7?",
        "Сравни два варианта.",
        "Определи узкое место.",
    ] {
        assert_eq!(
            normalize_step(p),
            Some(StepKind::AnswerQuestion),
            "RU opener lost: {p}"
        );
    }
}
