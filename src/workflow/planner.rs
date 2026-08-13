use crate::workflow::contract::{Step, StepKind, TaskClass};

/// English interrogative/auxiliary openers that mark a direct question.
const QUESTION_STARTERS_EN: &[&str] = &[
    "what", "why", "how", "which", "who", "whom", "when", "where", "is", "are", "was", "were",
    "does", "do", "did", "can", "could", "will", "would", "should",
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

/// Deterministic question detection (P0, H-2 fix).
///
/// A step text is a question when it opens with an interrogative (EN+RU),
/// opens with a compute imperative, ends with '?', or asks "сколько".
/// This runs AFTER CodeFix keyword pairs (so "can you find the bug?" stays
/// a CodeFix step) and BEFORE PlannerHardening keywords (so "what is a unit
/// test?" is a question, not planner hardening).
fn is_question(lower: &str, tokens: &[&str]) -> bool {
    let first = tokens.first().copied().unwrap_or("");
    QUESTION_STARTERS_EN.contains(&first)
        || QUESTION_STARTERS_RU.contains(&first)
        || COMPUTE_OPENERS_EN.contains(&first)
        || COMPUTE_OPENERS_RU.contains(&first)
        || lower.ends_with('?')
        || tokens
            .iter()
            .any(|t| strip_token_punctuation(t) == "сколько")
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

    // CodeFix step kinds (must precede PlannerHardening patterns to avoid
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
    // falling through to the ExecuteChanges default (P0, H-2 fix).
    if is_question(&lower, &tokens) {
        return Some(StepKind::AnswerQuestion);
    }

    if has("fallback") || has("error") {
        return Some(StepKind::AddLlmFallbackHandling);
    }

    if has("test") || contains_pair("test", "coverage") || contains_pair("write", "tests") {
        return Some(StepKind::AddPlannerTestCoverage);
    }

    if contains_pair("validate", "output") || contains_pair("sample", "tasks") {
        return Some(StepKind::ValidatePlannerOutput);
    }

    if contains_pair("prompt", "shape") || contains_pair("planner", "prompt") {
        return Some(StepKind::TightenPlannerPrompt);
    }

    if has("normalize") || has("filtering") || has("filter") {
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
