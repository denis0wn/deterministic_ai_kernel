use crate::workflow::contract::{Step, StepKind};

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

    let lower = cleaned.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();

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
pub fn apply_semantic_bias_from_seed(
    steps: Vec<StepKind>,
    seed: Option<u64>,
) -> Vec<StepKind> {
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

#[cfg(test)]
mod tests {
    use super::{
        apply_semantic_bias, normalize_step, parse_steps, validate_steps, SemanticBias, Step,
        StepKind,
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
}
