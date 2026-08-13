use deterministic_ai_kernel::workflow::contract::StepKind;
use deterministic_ai_kernel::workflow::semantic::interpreter::SeedInterpreter;
use proptest::prelude::*;
use std::collections::HashSet;

fn sample_domain() -> Vec<StepKind> {
    vec![
        StepKind::AnalyzeTask,
        StepKind::PlanExecution,
        StepKind::ExecuteChanges,
        StepKind::ValidatePlannerOutput,
    ]
}

fn domain_strings(domain: &[StepKind]) -> HashSet<String> {
    domain.iter().map(|k| format!("{:?}", k)).collect()
}

proptest! {
    #[test]
    fn same_seed_gives_same_bias(seed in any::<u64>()) {
        let domain = sample_domain();
        let a = SeedInterpreter::interpret(seed, &domain);
        let b = SeedInterpreter::interpret(seed, &domain);
        prop_assert_eq!(a, b);
    }

    #[test]
    fn bias_never_expands_step_set(seed in any::<u64>()) {
        let domain = sample_domain();
        let allowed = domain_strings(&domain);
        let bias = SeedInterpreter::interpret(seed, &domain);

        // weights keys must be subset of domain strings
        prop_assert!(bias.weights.keys().all(|k| allowed.contains(k)));
        // preferred must be subset of domain
        let allowed_kind: HashSet<_> = domain.iter().cloned().collect();
        prop_assert!(bias.preferred.iter().all(|k| allowed_kind.contains(k)));
    }

    #[test]
    fn preferred_is_subset_without_duplicates(seed in any::<u64>()) {
        let domain = sample_domain();
        let bias = SeedInterpreter::interpret(seed, &domain);
        let preferred: HashSet<_> = bias.preferred.iter().cloned().collect();

        prop_assert_eq!(preferred.len(), bias.preferred.len());
        prop_assert!(preferred.len() <= domain.len());
    }

    #[test]
    fn different_seeds_are_allowed_to_diverge(a in any::<u64>(), b in any::<u64>()) {
        let domain = sample_domain();
        let left = SeedInterpreter::interpret(a, &domain);
        let right = SeedInterpreter::interpret(b, &domain);

        if a == b {
            prop_assert_eq!(left, right);
        }
    }
}
