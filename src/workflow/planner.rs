use crate::workflow::contract::{Step, StepKind, TaskClass};

/// English interrogative/auxiliary openers that mark a direct question.
const QUESTION_STARTERS_EN: &[&str] = &[
    "what", "why", "how", "which", "who", "whom", "when", "where", "is", "are", "was", "were",
    "does", "do", "did", "can", "could", "will", "would", "should",
];

/// English analytical-imperative openers (R2): tasks that demand an
/// analytical answer, not code changes.
const ANALYTICAL_OPENERS_EN: &[&str] = &[
    "determine",
    "evaluate",
    "explain",
    "compare",
    "analyze",
    "describe",
];

/// Russian interrogative openers that mark a direct question.
const QUESTION_STARTERS_RU: &[&str] = &[
    "сколько",
    "что",
    "чему",
    "какой",
    "какая",
    "какое",
    "какие",
    "кто",
    "когда",
    "где",
    "почему",
    "зачем",
    "как",
];

/// Russian analytical-imperative openers (R2): tasks that demand an
/// analytical answer, not code changes. "столько" covers the common
/// colloquial misspelling of "сколько" ("Столько будет 9 умножить на 7?").
/// HD-1: read/analyze/verify intent verbs added — they demand an answer,
/// not file mutations ("проверь файл", "опиши содержимое лога",
/// "назови инвентарный номер", "прочитай README и объясни архитектуру").
const ANALYTICAL_OPENERS_RU: &[&str] = &[
    "есть",
    "можно",
    "объясни",
    "укажи",
    "столько",
    "сравни",
    "определи",
    "проанализируй",
    "проверь",
    "опиши",
    "назови",
    "скажи",
    "прочитай",
    "расскажи",
];

/// Explicit no-change constraints (HD-1). A payload that FORBIDS mutations
/// is by definition an analysis/question: it must reach the model through
/// AnswerQuestion, never the execute_changes frame. Checked after CodeFix
/// keyword pairs, so explicit step flows ("apply patch", "run tests") are
/// never hijacked. R3: extended with "не запускай"/"не меняй"/"без запуска"
/// and EN "do not run"/"without executing"/"without running" per spec.
const NEGATIVE_CONSTRAINT_MARKERS_RU: &[&str] = &[
    "не выполняй никаких изменений",
    "не выполняй изменений",
    "без выполнения каких-либо команд",
    "без выполнения команд",
    "без изменений",
    "без запуска",
    "не изменяй",
    "не меняй",
    "не меняя",
    "не исправляй",
    "не применяй",
    "не запускай",
    "не вноси изменения",
];
const NEGATIVE_CONSTRAINT_MARKERS_EN: &[&str] = &[
    "do not modify",
    "do not change",
    "do not apply",
    "do not run",
    "without modifying",
    "without making changes",
    "without applying",
    "without executing",
    "without running",
    "don't modify",
    "don't change",
    "don't apply",
    "don't run",
    "no file changes",
];

/// Analytical claim-check phrases that can appear MID-text (HD-1). The
/// opener rules cannot see them when a declarative sentence comes first
/// ("Товар стоит 100 рублей... Проверь утверждение.").
const ANALYTICAL_PHRASES_RU: &[&str] = &["проверь утверждение", "верно ли", "правда ли"];
const ANALYTICAL_PHRASES_EN: &[&str] = &["check the claim", "check whether", "is the claim"];

/// Imperative compute openers (EN+RU): tasks that demand a concrete value,
/// not code changes.
const COMPUTE_OPENERS_EN: &[&str] = &["calculate", "compute"];
const COMPUTE_OPENERS_RU: &[&str] = &[
    "вычисли",
    "вычислите",
    "посчитай",
    "посчитайте",
    "рассчитай",
    "рассчитайте",
];

fn strip_token_punctuation(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric())
}

/// Deterministic question detection (P0, H-2 fix; R2 extension; HD-1).
///
/// A step text is a question/analytical task when it opens with an
/// interrogative (EN+RU), opens with a compute/analytical imperative,
/// contains a '?' anywhere, asks "сколько", explicitly forbids file
/// changes (negative constraints), or contains an analytical claim-check
/// phrase mid-text. A '?' anywhere (not just at the end) counts because
/// multi-sentence analytical tasks often embed the question mid-text and
/// end with formatting instructions.
/// This runs AFTER CodeFix keyword pairs (so "can you find the bug?" stays
/// a CodeFix step and "run tests but do not modify" stays RunTests) and
/// BEFORE PlannerHardening keywords (so "what is a unit test?" is a
/// question, not planner hardening).
fn is_question(lower: &str, tokens: &[&str]) -> bool {
    let first = tokens.first().copied().unwrap_or("");
    QUESTION_STARTERS_EN.contains(&first)
        || ANALYTICAL_OPENERS_EN.contains(&first)
        || QUESTION_STARTERS_RU.contains(&first)
        || ANALYTICAL_OPENERS_RU.contains(&first)
        || COMPUTE_OPENERS_EN.contains(&first)
        || COMPUTE_OPENERS_RU.contains(&first)
        || lower.contains('?')
        || tokens
            .iter()
            .any(|t| strip_token_punctuation(t) == "сколько")
        || NEGATIVE_CONSTRAINT_MARKERS_RU
            .iter()
            .any(|m| lower.contains(m))
        || NEGATIVE_CONSTRAINT_MARKERS_EN
            .iter()
            .any(|m| lower.contains(m))
        || ANALYTICAL_PHRASES_RU.iter().any(|m| lower.contains(m))
        || ANALYTICAL_PHRASES_EN.iter().any(|m| lower.contains(m))
}

pub fn normalize_step(raw: &str) -> Option<StepKind> {
    let cleaned = raw
        .trim()
        .trim_start_matches(|c: char| {
            c.is_ascii_digit() || c == '.' || c == '-' || c == ')' || c.is_whitespace()
        })
        .trim()
        .trim_matches('`')
        .trim()
        .trim_end_matches('.');

    if cleaned.is_empty() {
        return None;
    }

    // Full Unicode lowercase (not to_ascii_lowercase): Russian question
    // detection requires Cyrillic capitals to fold ("Сколько" -> "сколько").
    // For pure-ASCII inputs this is identical to the previous behavior.
    let lower = cleaned.to_lowercase();
    // Tokens are stripped of surrounding punctuation so keyword matching
    // survives real payloads ("patch code, run tests" -> "code", not
    // "code,"). Without this, CodeFix pairs silently miss on commas/periods
    // (observed live in the P1 CodeFix run).
    let tokens: Vec<&str> = lower
        .split_whitespace()
        .map(strip_token_punctuation)
        .filter(|t| !t.is_empty())
        .collect();

    let has = |needle: &str| tokens.contains(&needle);
    let contains_pair = |a: &str, b: &str| has(a) && has(b);

    // CodeFix step kinds (must precede question detection — "can you find
    // the bug?" stays LocateBug — and PlannerHardening patterns to avoid
    // "test"/"validate" keyword collisions).
    if contains_pair("read", "repository") || contains_pair("read", "repo") {
        return Some(StepKind::ReadRepository);
    }

    if contains_pair("locate", "bug") || contains_pair("find", "bug") {
        return Some(StepKind::LocateBug);
    }

    if contains_pair("patch", "code") || contains_pair("fix", "code") {
        return Some(StepKind::PatchCode);
    }

    // P2: applying a validated patch is a kernel-only effect step, distinct
    // from generating one. It was previously folded into PatchCode, which
    // re-invoked the LLM instead of applying anything.
    if contains_pair("apply", "patch") {
        return Some(StepKind::ApplyPatch);
    }

    if contains_pair("run", "tests") {
        return Some(StepKind::RunTests);
    }

    if contains_pair("validate", "patch") {
        return Some(StepKind::ValidatePatch);
    }

    // Interrogative/analytical questions route to the answer flow instead of
    // falling through to the ExecuteChanges default (P0, H-2 fix). HD-1:
    // this now also runs BEFORE the docs/deploy reject list, so analytical
    // payloads that merely mention "readme"/"documentation" ("прочитай
    // README и объясни архитектуру") reach the model as questions instead
    // of falling into the execute_changes frame via the reject-to-default
    // path.
    if is_question(&lower, &tokens) {
        return Some(StepKind::AnswerQuestion);
    }

    if has("deploy")
        || has("research")
        || has("ux")
        || has("readme")
        || has("documentation")
        || contains_pair("user", "interface")
    {
        return None;
    }

    if contains_pair("parse", "cli") || contains_pair("parse", "arguments") {
        return None;
    }

    // Planner-hardening meta steps are computed stubs (the model is never
    // asked), so ALL their keywords and keyword pairs fire ONLY when the
    // text actually references planner/pipeline/kernel/LLM infrastructure.
    // Without this gate, domain tasks merely mentioning "test"/"error"/
    // "normalize"/"filter"/"test coverage"/"validate output" were silently
    // routed to stub steps and the model was bypassed entirely (defect R1;
    // R2 forensic verification proved the ungated pairs were still a hole:
    // "describe the test coverage of the module" → AddPlannerTestCoverage).
    let planner_context = has("planner") || has("pipeline") || has("kernel") || has("llm");
    if planner_context && (has("fallback") || has("error")) {
        return Some(StepKind::AddLlmFallbackHandling);
    }

    if planner_context
        && (contains_pair("test", "coverage") || contains_pair("write", "tests") || has("test"))
    {
        return Some(StepKind::AddPlannerTestCoverage);
    }

    if planner_context && (contains_pair("validate", "output") || contains_pair("sample", "tasks"))
    {
        return Some(StepKind::ValidatePlannerOutput);
    }

    if planner_context && (contains_pair("prompt", "shape") || contains_pair("planner", "prompt")) {
        return Some(StepKind::TightenPlannerPrompt);
    }

    if planner_context && (has("normalize") || has("filtering") || has("filter")) {
        return Some(StepKind::NormalizePlannerOutput);
    }

    None
}

pub fn parse_steps(text: &str) -> Vec<StepKind> {
    let mut out = Vec::new();

    for line in text.lines() {
        if let Some(kind) = normalize_step(line) {
            if !out.contains(&kind) {
                out.push(kind);
            }
        }
    }

    out
}

/// Deterministic preference ordering for planner step kinds.
///
/// Contract:
/// - `preferred` is only an ordering hint.
/// - applying a bias must never add, remove, or duplicate `StepKind`s.
/// - different seeds may choose different stable orderings, but the same seed
///   must always produce the same preference vector.
#[derive(Clone, Debug, Default, PartialEq)]
// moved to workflow::semantic::bias::SemanticBias
pub struct SemanticBias {
    pub preferred: Vec<StepKind>,
}

/// Maps a replay-safe seed to a deterministic semantic bias.
///
/// This function is intentionally small and pure: seed in, preference order out.
/// It must remain stable for identical seeds across runs unless the bias contract
/// is intentionally versioned and migrated.
#[cfg_attr(not(test), allow(dead_code))]
pub fn seed_to_bias(seed: u64) -> SemanticBias {
    let preferred = if seed.is_multiple_of(2) {
        vec![
            StepKind::TightenPlannerPrompt,
            StepKind::NormalizePlannerOutput,
            StepKind::AddLlmFallbackHandling,
            StepKind::AddPlannerTestCoverage,
            StepKind::ValidatePlannerOutput,
        ]
    } else {
        vec![
            StepKind::NormalizePlannerOutput,
            StepKind::TightenPlannerPrompt,
            StepKind::AddLlmFallbackHandling,
            StepKind::AddPlannerTestCoverage,
            StepKind::ValidatePlannerOutput,
        ]
    };

    SemanticBias { preferred }
}

/// Applies a stable preference ordering over planner `StepKind`s.
///
/// Contract:
/// - `None` is a no-op.
/// - output is a stable reordering of the input.
/// - membership and cardinality are preserved exactly.
/// - canonical planner meaning is not enforced here; that is the job of
///   `validate_steps`.
pub fn apply_semantic_bias_from_seed(steps: Vec<StepKind>, seed: Option<u64>) -> Vec<StepKind> {
    let bias = seed.map(seed_to_bias);
    apply_semantic_bias(steps, bias)
}

pub fn apply_semantic_bias(steps: Vec<StepKind>, bias: Option<SemanticBias>) -> Vec<StepKind> {
    let Some(bias) = bias else {
        return steps;
    };

    let mut weighted: Vec<(usize, StepKind)> = steps.into_iter().enumerate().collect();

    weighted.sort_by(|a, b| {
        let (ia, ka) = a;
        let (ib, kb) = b;

        let wa = bias
            .preferred
            .iter()
            .position(|k| k == ka)
            .unwrap_or(usize::MAX);
        let wb = bias
            .preferred
            .iter()
            .position(|k| k == kb)
            .unwrap_or(usize::MAX);

        wa.cmp(&wb).then_with(|| ia.cmp(ib))
    });

    weighted.into_iter().map(|(_, kind)| kind).collect()
}

/// Projects planner steps onto canonical planner order.
///
/// Contract:
/// - validation may reorder and deduplicate by canonical workflow order.
/// - unlike `apply_semantic_bias`, this function operates on full `Step` values.
/// - if no canonical planner step is found, the original input is returned.
pub fn validate_steps(steps: Vec<Step>) -> Vec<Step> {
    let canonical_order = [
        StepKind::TightenPlannerPrompt,
        StepKind::NormalizePlannerOutput,
        StepKind::AddLlmFallbackHandling,
        StepKind::AddPlannerTestCoverage,
        StepKind::ValidatePlannerOutput,
    ];

    let mut validated = Vec::new();

    for kind in canonical_order {
        if let Some(step) = steps.iter().find(|s| s.kind == kind) {
            if !validated
                .iter()
                .any(|existing: &Step| existing.kind == step.kind)
            {
                validated.push(step.clone());
            }
        }
    }

    if validated.is_empty() {
        return steps;
    }

    validated
}

/// Deterministic task-intent classification for the production pipeline.
///
/// Kernel-owned: no LLM input is consulted. A plan whose steps are ALL
/// interrogative/answer steps is a `Question` task; anything else keeps the
/// conservative `Generic` classification (P0, H-2 fix). Empty plans are
/// Generic so the existing empty-plan error surfaces stay unchanged.
pub fn classify_task_class(step_texts: &[String]) -> TaskClass {
    if !step_texts.is_empty()
        && step_texts
            .iter()
            .all(|text| normalize_step(text) == Some(StepKind::AnswerQuestion))
    {
        TaskClass::Question
    } else {
        TaskClass::Generic
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_semantic_bias, classify_task_class, normalize_step, parse_steps, validate_steps,
        SemanticBias, Step, StepKind,
    };
    use crate::workflow::planner::seed_to_bias;
    use proptest::prelude::*;

    #[test]
    fn normalize_step_maps_fallback_language() {
        assert_eq!(
            normalize_step("Implement fallback logic for planner errors"),
            Some(StepKind::AddLlmFallbackHandling)
        );
    }

    #[test]
    fn normalize_step_maps_test_language() {
        assert_eq!(
            normalize_step("Write tests for planner output"),
            Some(StepKind::AddPlannerTestCoverage)
        );
    }

    #[test]
    fn normalize_step_rejects_deployment_language() {
        assert_eq!(normalize_step("Deploy planner changes"), None);
    }

    #[test]
    fn parse_steps_extracts_unique_kinds_in_order() {
        let kinds = parse_steps(
            "tighten planner prompt\nnormalize planner output\nnormalize planner output\nadd llm fallback handling"
        );

        assert_eq!(
            kinds,
            vec![
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
                StepKind::AddLlmFallbackHandling,
            ]
        );
    }

    #[test]
    fn validate_steps_dedupes_and_orders_canonical_steps() {
        let steps = vec![
            Step {
                kind: StepKind::ValidatePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::AddPlannerTestCoverage,
                detail: None,
            },
            Step {
                kind: StepKind::NormalizePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::AddPlannerTestCoverage,
                detail: None,
            },
            Step {
                kind: StepKind::TightenPlannerPrompt,
                detail: None,
            },
        ];

        let validated = validate_steps(steps);
        let kinds: Vec<StepKind> = validated.into_iter().map(|s| s.kind).collect();

        assert_eq!(
            kinds,
            vec![
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
                StepKind::AddPlannerTestCoverage,
                StepKind::ValidatePlannerOutput,
            ]
        );
    }

    #[test]
    fn validate_steps_is_invariant_under_input_permutation() {
        let a = vec![
            Step {
                kind: StepKind::TightenPlannerPrompt,
                detail: None,
            },
            Step {
                kind: StepKind::NormalizePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::AddPlannerTestCoverage,
                detail: None,
            },
            Step {
                kind: StepKind::ValidatePlannerOutput,
                detail: None,
            },
        ];

        let b = vec![
            Step {
                kind: StepKind::ValidatePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::AddPlannerTestCoverage,
                detail: None,
            },
            Step {
                kind: StepKind::NormalizePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::TightenPlannerPrompt,
                detail: None,
            },
        ];

        let a_validated: Vec<StepKind> = validate_steps(a).into_iter().map(|s| s.kind).collect();
        let b_validated: Vec<StepKind> = validate_steps(b).into_iter().map(|s| s.kind).collect();

        assert_eq!(a_validated, b_validated);
    }

    #[test]
    fn apply_semantic_bias_none_is_noop() {
        let steps = vec![
            StepKind::TightenPlannerPrompt,
            StepKind::NormalizePlannerOutput,
            StepKind::AddPlannerTestCoverage,
        ];

        assert_eq!(apply_semantic_bias(steps.clone(), None), steps);
    }

    #[test]
    fn semantic_bias_none_preserves_identity_for_all_known_permutations() {
        let base = vec![
            StepKind::NormalizePlannerOutput,
            StepKind::TightenPlannerPrompt,
            StepKind::AddLlmFallbackHandling,
            StepKind::AddPlannerTestCoverage,
            StepKind::ValidatePlannerOutput,
        ];

        let permutations = vec![
            base.clone(),
            vec![
                StepKind::ValidatePlannerOutput,
                StepKind::AddPlannerTestCoverage,
                StepKind::AddLlmFallbackHandling,
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
            ],
            vec![
                StepKind::AddLlmFallbackHandling,
                StepKind::NormalizePlannerOutput,
                StepKind::ValidatePlannerOutput,
                StepKind::TightenPlannerPrompt,
                StepKind::AddPlannerTestCoverage,
            ],
        ];

        for steps in permutations {
            assert_eq!(apply_semantic_bias(steps.clone(), None), steps);
        }
    }

    #[test]
    fn semantic_bias_preserves_permutation_closure_across_seed_matrix() {
        let inputs = vec![
            vec![
                StepKind::NormalizePlannerOutput,
                StepKind::TightenPlannerPrompt,
                StepKind::AddLlmFallbackHandling,
                StepKind::AddPlannerTestCoverage,
                StepKind::ValidatePlannerOutput,
            ],
            vec![
                StepKind::ValidatePlannerOutput,
                StepKind::AddPlannerTestCoverage,
                StepKind::AddLlmFallbackHandling,
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
            ],
            vec![
                StepKind::AddPlannerTestCoverage,
                StepKind::NormalizePlannerOutput,
                StepKind::ValidatePlannerOutput,
                StepKind::AddLlmFallbackHandling,
                StepKind::TightenPlannerPrompt,
            ],
        ];

        for seed in [0_u64, 1, 2, 7, 42, 99, 1024, u64::MAX] {
            let bias = seed_to_bias(seed);
            for steps in &inputs {
                let output = apply_semantic_bias(steps.clone(), Some(bias.clone()));

                for step in steps {
                    assert!(
                        output.contains(step),
                        "seed={seed} must preserve every input step"
                    );
                }
                assert_eq!(
                    output.len(),
                    steps.len(),
                    "seed={seed} must preserve length"
                );
            }
        }
    }

    #[test]
    fn semantic_bias_is_deterministic_across_seed_and_input_matrix() {
        let inputs = vec![
            vec![
                StepKind::NormalizePlannerOutput,
                StepKind::TightenPlannerPrompt,
                StepKind::AddLlmFallbackHandling,
                StepKind::AddPlannerTestCoverage,
                StepKind::ValidatePlannerOutput,
            ],
            vec![
                StepKind::ValidatePlannerOutput,
                StepKind::AddPlannerTestCoverage,
                StepKind::AddLlmFallbackHandling,
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
            ],
            vec![
                StepKind::AddLlmFallbackHandling,
                StepKind::ValidatePlannerOutput,
                StepKind::NormalizePlannerOutput,
                StepKind::AddPlannerTestCoverage,
                StepKind::TightenPlannerPrompt,
            ],
        ];

        for seed in [0_u64, 1, 2, 7, 42, 99, 1024, u64::MAX] {
            let bias = seed_to_bias(seed);
            for steps in &inputs {
                let out1 = apply_semantic_bias(steps.clone(), Some(bias.clone()));
                let out2 = apply_semantic_bias(steps.clone(), Some(bias.clone()));
                assert_eq!(out1, out2, "seed={seed} must be deterministic");
            }
        }
    }

    #[test]
    fn semantic_bias_preserves_canonical_validation_result() {
        let inputs = vec![
            vec![
                StepKind::NormalizePlannerOutput,
                StepKind::TightenPlannerPrompt,
                StepKind::AddLlmFallbackHandling,
                StepKind::AddPlannerTestCoverage,
                StepKind::ValidatePlannerOutput,
            ],
            vec![
                StepKind::ValidatePlannerOutput,
                StepKind::AddPlannerTestCoverage,
                StepKind::AddLlmFallbackHandling,
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
            ],
        ];

        for seed in [0_u64, 1, 2, 7, 42, 99, 1024, u64::MAX] {
            let bias = seed_to_bias(seed);
            for steps in &inputs {
                let biased = apply_semantic_bias(steps.clone(), Some(bias.clone()));

                assert_eq!(
                    biased.len(),
                    steps.len(),
                    "seed={seed} must preserve validated cardinality"
                );
                for step in steps {
                    assert!(
                        biased.contains(step),
                        "seed={seed} must preserve validated step membership"
                    );
                }
            }
        }
    }

    #[test]
    fn seed_to_bias_replay_safe() {
        let seeds = [0u64, 1, 42, u64::MAX];
        for seed in seeds {
            let a = seed_to_bias(seed);
            let b = seed_to_bias(seed);
            assert_eq!(a.preferred, b.preferred);
        }
    }

    proptest::proptest! {
        #[test]
        fn semantic_bias_preserves_membership_and_length_for_generated_inputs(
            seed in any::<u64>(),
            steps in proptest::collection::vec(
                proptest::sample::select(vec![
                    StepKind::TightenPlannerPrompt,
                    StepKind::NormalizePlannerOutput,
                    StepKind::AddLlmFallbackHandling,
                    StepKind::AddPlannerTestCoverage,
                    StepKind::ValidatePlannerOutput,
                ]),
                0..12
            )
        ) {
            let bias = seed_to_bias(seed);
            let output = apply_semantic_bias(steps.clone(), Some(bias));
            prop_assert_eq!(output.len(), steps.len());
            for step in &steps {
                prop_assert!(output.contains(step));
            }
        }

        #[test]
        fn semantic_bias_none_is_identity_for_generated_inputs(
            steps in proptest::collection::vec(
                proptest::sample::select(vec![
                    StepKind::TightenPlannerPrompt,
                    StepKind::NormalizePlannerOutput,
                    StepKind::AddLlmFallbackHandling,
                    StepKind::AddPlannerTestCoverage,
                    StepKind::ValidatePlannerOutput,
                ]),
                0..12
            )
        ) {
            let output = apply_semantic_bias(steps.clone(), None);
            prop_assert_eq!(output, steps);
        }
    }

    #[test]
    fn apply_semantic_bias_stably_orders_by_weight() {
        let steps = vec![
            StepKind::ValidatePlannerOutput,
            StepKind::TightenPlannerPrompt,
            StepKind::NormalizePlannerOutput,
        ];
        let bias = SemanticBias {
            preferred: vec![
                StepKind::NormalizePlannerOutput,
                StepKind::TightenPlannerPrompt,
            ],
        };

        let out = apply_semantic_bias(steps, Some(bias));
        assert_eq!(
            out,
            vec![
                StepKind::NormalizePlannerOutput,
                StepKind::TightenPlannerPrompt,
                StepKind::ValidatePlannerOutput,
            ]
        );
    }

    #[test]
    fn validate_steps_ignores_duplicate_permutations() {
        let steps = vec![
            Step {
                kind: StepKind::ValidatePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::NormalizePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::ValidatePlannerOutput,
                detail: None,
            },
            Step {
                kind: StepKind::TightenPlannerPrompt,
                detail: None,
            },
            Step {
                kind: StepKind::AddPlannerTestCoverage,
                detail: None,
            },
            Step {
                kind: StepKind::NormalizePlannerOutput,
                detail: None,
            },
        ];

        let kinds: Vec<StepKind> = validate_steps(steps).into_iter().map(|s| s.kind).collect();

        assert_eq!(
            kinds,
            vec![
                StepKind::TightenPlannerPrompt,
                StepKind::NormalizePlannerOutput,
                StepKind::AddPlannerTestCoverage,
                StepKind::ValidatePlannerOutput,
            ]
        );
    }

    #[test]
    fn normalize_step_maps_codefix_step_kinds() {
        assert_eq!(
            normalize_step("Read Repository"),
            Some(StepKind::ReadRepository)
        );
        assert_eq!(normalize_step("Locate Bug"), Some(StepKind::LocateBug));
        assert_eq!(normalize_step("Patch Code"), Some(StepKind::PatchCode));
        assert_eq!(normalize_step("Run Tests"), Some(StepKind::RunTests));
        assert_eq!(
            normalize_step("Validate Patch"),
            Some(StepKind::ValidatePatch)
        );
    }

    #[test]
    fn normalize_step_codefix_patterns_precede_planner_hardening() {
        // "Run Tests" must not match AddPlannerTestCoverage
        assert_eq!(normalize_step("Run Tests"), Some(StepKind::RunTests));
        // "Validate Patch" must not match ValidatePlannerOutput
        assert_eq!(
            normalize_step("Validate Patch"),
            Some(StepKind::ValidatePatch)
        );
    }

    #[test]
    fn normalize_step_does_not_misroute_domain_keyword_mentions_r1() {
        // R1 regression: domain tasks that merely mention "test"/"error"/
        // "normalize"/"filter" must NOT be routed to planner-hardening stub
        // steps (which never ask the model). Without planner context they
        // fall through to normal planning (None -> ExecuteChanges).
        let b2 = "Five machines produce parts. Exactly one machine produces defective parts. \
                  You have one test that identifies whether a batch contains a defect. \
                  Design the minimum-test strategy if the machines can be grouped.";
        assert_eq!(normalize_step(b2), None);
        assert_eq!(
            normalize_step("handle the error case in the calculation"),
            None
        );
        assert_eq!(normalize_step("normalize the readings from sensor 4"), None);
        assert_eq!(normalize_step("filter the results by date"), None);
        // Explicit hardening phrasings still route (context-rich pairs).
        assert_eq!(
            normalize_step("write tests for planner output"),
            Some(StepKind::AddPlannerTestCoverage)
        );
        assert_eq!(
            normalize_step("add test coverage for the planner"),
            Some(StepKind::AddPlannerTestCoverage)
        );
        // Single-word keywords route only with planner context.
        assert_eq!(
            normalize_step("add fallback for llm errors"),
            Some(StepKind::AddLlmFallbackHandling)
        );
        assert_eq!(
            normalize_step("normalize the planner output"),
            Some(StepKind::NormalizePlannerOutput)
        );
    }

    #[test]
    fn normalize_step_routes_analytical_imperatives_to_answers_r2() {
        // R2 regression: analytical tasks in imperative/declarative form
        // must reach the model through AnswerQuestion, not the code-executor
        // frame. '?' anywhere in the text counts (A4 ends with formatting
        // instructions, not '?').
        let a4 = "A price is increased by 18% and then discounted by 18%. Is the final price \
                  equal to the original price? Give the exact calculation and a yes/no answer.";
        assert_eq!(normalize_step(a4), Some(StepKind::AnswerQuestion));
        // "сколько" mid-text (I1).
        let i1 = "Есть 120 деталей. 25% отправили на склад A. Сколько деталей осталось?";
        assert_eq!(normalize_step(i1), Some(StepKind::AnswerQuestion));
        // Analytical openers (J1 "есть", D2 "можно", D3 "объясни", conc_B "столько").
        let j1 = "Есть 4 задачи с длительностями 3, 5, 2 и 7 часов и две одинаковые машины. \
                  Укажи распределение и итоговый makespan.";
        assert_eq!(normalize_step(j1), Some(StepKind::AnswerQuestion));
        assert_eq!(
            normalize_step("Можно ли выполнить все три операции за 8 часов?"),
            Some(StepKind::AnswerQuestion)
        );
        assert_eq!(
            normalize_step("Объясни на русском, чем отличается retryable failure от terminal."),
            Some(StepKind::AnswerQuestion)
        );
        assert_eq!(
            normalize_step("Столько будет 9 умножить на 7? Ответь числом."),
            Some(StepKind::AnswerQuestion)
        );
        assert_eq!(
            normalize_step("Determine the total driving time for the route."),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_routes_hd1_acceptance_payloads_to_answers() {
        // HD-1 regression: the 2026-08-14 acceptance misrouted these 12
        // analytical payloads to execute_changes. Every one must route to
        // AnswerQuestion: read/analyze/verify intent verbs (A), explicit
        // no-change constraints (B), and mid-text claim-check phrases (C).
        // ── A: analytical intent verbs ──
        let cases = [
            "Проанализируй, почему конвейер может простаивать.",
            "Проверь файл и скажи, что в нём неправильно.",
            "Скажи, какой алгоритм быстрее в среднем.",
            "Прочитай README и объясни архитектуру.",
            "Опиши, как работает конвейер CI/CD.",
            "Опиши содержимое лога последней смены.",
            "Назови инвентарный номер машины 3.",
            "Analyze why the pipeline may be idle.",
            "Describe the contents of the last shift log.",
        ];
        for p in cases {
            assert_eq!(
                normalize_step(p),
                Some(StepKind::AnswerQuestion),
                "HD-1 misroute: {p}"
            );
        }
        // ── B: explicit no-change constraints ──
        let constraints = [
            "Найди причину ошибки в коде, но не исправляй файл.",
            "Найди файл конфигурации, но не изменяй его.",
            "Подготовь patch для исправления, но не применяй его.",
            "Не выполняй никаких изменений, только проанализируй код.",
            "Опиши архитектуру без выполнения каких-либо команд.",
            "Review the config file, but do not modify it.",
        ];
        for p in constraints {
            assert_eq!(
                normalize_step(p),
                Some(StepKind::AnswerQuestion),
                "HD-1 negative constraint ignored: {p}"
            );
        }
        // ── C: mid-text claim-check phrases ──
        let a11 = "Товар стоит 100 рублей. После скидки 20% он стоит 90 рублей. \
                   Проверь утверждение.";
        assert_eq!(normalize_step(a11), Some(StepKind::AnswerQuestion));
    }

    #[test]
    fn normalize_step_hd1_does_not_hijack_codefix_or_imperatives() {
        // HD-1 must not pull explicit CodeFix steps into the answer flow,
        // even when they co-occur with negative-constraint wording, and
        // plain mutation imperatives stay on the execute path.
        assert_eq!(
            normalize_step("run tests but do not modify anything"),
            Some(StepKind::RunTests)
        );
        assert_eq!(
            normalize_step("apply patch and validate patch"),
            Some(StepKind::ApplyPatch)
        );
        assert_eq!(
            normalize_step("Can you find the bug in the parser?"),
            Some(StepKind::LocateBug)
        );
        assert_eq!(normalize_step("Refactor the scheduler module"), None);
        assert_eq!(normalize_step("Deploy planner changes"), None);
        assert_eq!(
            normalize_step("Describe the test coverage of the module"),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_r3_adversarial_negative_constraints_ru_en() {
        // R3 step 5: NEW adversarial negative-constructive cases (RU+EN)
        // beyond the 12 acceptance payloads. Every one carries an explicit
        // no-run/no-change constraint and MUST route to AnswerQuestion even
        // when action verbs ("найди", "проверь", "review", "analyze")
        // co-occur in the same sentence.
        let cases = [
            // RU: "не запускай" family
            "Объясни причину сбоя конвейера, но не запускай никаких команд.",
            "Проверь конфигурацию и найди расхождение, не запуская сервис.",
            // RU: "не меняй"/"не меняя" family
            "Проверь логи и найди ошибку, не меняя файлы.",
            "Найди узкое место в расписании, не меняя сам план.",
            // RU: "без запуска"
            "Расскажи про архитектуру ядра без запуска каких-либо процессов.",
            // EN: "do not run" family
            "Describe the deployment procedure, but do not run anything.",
            "Analyze the stack trace and do not run the service.",
            // EN: "without executing"/"without running"
            "Analyze the memory usage without executing any commands.",
            "Review the scheduler code without running the service.",
        ];
        for p in cases {
            assert_eq!(
                normalize_step(p),
                Some(StepKind::AnswerQuestion),
                "R3 adversarial negative constraint ignored: {p}"
            );
        }
        // Constructive control: the SAME verbs without any constraint stay
        // on the execute path — negative markers must not leak into plain
        // imperative routing.
        assert_eq!(normalize_step("Найди узкое место и перестрой план"), None);
        assert_eq!(
            normalize_step("Review the scheduler code and refactor it"),
            None
        );
    }

    #[test]
    fn normalize_step_codefix_variants() {
        assert_eq!(normalize_step("read repo"), Some(StepKind::ReadRepository));
        assert_eq!(
            normalize_step("find bug in parser"),
            Some(StepKind::LocateBug)
        );
        assert_eq!(
            normalize_step("apply patch to auth module"),
            Some(StepKind::ApplyPatch)
        );
        assert_eq!(
            normalize_step("patch code in auth module"),
            Some(StepKind::PatchCode)
        );
    }

    // ── P0 (H-2): question detection ───────────────────────────────────────

    #[test]
    fn normalize_step_maps_english_arithmetic_question() {
        assert_eq!(
            normalize_step("What is 17 × 19?"),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_maps_russian_question_from_acceptance() {
        assert_eq!(
            normalize_step(
                "На складе было 7 насосов, 3 забрали. Сколько осталось? Ответь по-русски."
            ),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_maps_russian_compute_imperative() {
        assert_eq!(
            normalize_step("Вычисли сумму чисел от 1 до 10."),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_maps_english_compute_imperative() {
        assert_eq!(
            normalize_step("Calculate the total throughput over 8 hours"),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_question_mark_alone_marks_question() {
        assert_eq!(
            normalize_step("The line rate is 90 units per hour, correct?"),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_codefix_pairs_precede_question_detection() {
        // "can you find the bug?" must stay a CodeFix step, not a question.
        assert_eq!(
            normalize_step("Can you find the bug in auth?"),
            Some(StepKind::LocateBug)
        );
        assert_eq!(
            normalize_step("Please run tests now?"),
            Some(StepKind::RunTests)
        );
    }

    #[test]
    fn normalize_step_questions_precede_planner_hardening_keywords() {
        // "what is a unit test?" is a question, not AddPlannerTestCoverage.
        assert_eq!(
            normalize_step("What is a unit test?"),
            Some(StepKind::AnswerQuestion)
        );
    }

    #[test]
    fn normalize_step_imperatives_are_not_questions() {
        assert_eq!(normalize_step("Refactor the scheduler module"), None);
        assert_eq!(
            normalize_step("Write tests for planner output"),
            Some(StepKind::AddPlannerTestCoverage)
        );
    }

    #[test]
    fn classify_task_class_question_payload() {
        let steps = vec!["What is 17 × 19?".to_string()];
        assert_eq!(
            classify_task_class(&steps),
            crate::workflow::contract::TaskClass::Question
        );
    }

    #[test]
    fn classify_task_class_generic_payload() {
        let steps = vec!["Refactor the parser and add tests".to_string()];
        assert_eq!(
            classify_task_class(&steps),
            crate::workflow::contract::TaskClass::Generic
        );
    }

    #[test]
    fn classify_task_class_mixed_plan_is_generic() {
        let steps = vec![
            "What is the bottleneck?".to_string(),
            "Patch code to fix it".to_string(),
        ];
        assert_eq!(
            classify_task_class(&steps),
            crate::workflow::contract::TaskClass::Generic
        );
    }

    #[test]
    fn classify_task_class_empty_plan_is_generic() {
        assert_eq!(
            classify_task_class(&[]),
            crate::workflow::contract::TaskClass::Generic
        );
    }
}
