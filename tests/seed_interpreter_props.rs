use deterministic_ai_kernel::workflow::{
    contract::StepKind, semantic::interpreter::SeedInterpreter,
};

fn domain() -> Vec<StepKind> {
    vec![
        StepKind::AnalyzeTask,
        StepKind::PlanExecution,
        StepKind::ExecuteChanges,
        StepKind::RunTests,
        StepKind::ValidatePatch,
    ]
}

#[test]
fn same_seed_produces_identical_bias() {
    let domain = domain();
    let a = SeedInterpreter::interpret(42, &domain);
    let b = SeedInterpreter::interpret(42, &domain);
    assert_eq!(a, b);
}

#[test]
fn different_seeds_produce_different_orderings() {
    let domain = domain();
    let a = SeedInterpreter::interpret(1, &domain);
    let b = SeedInterpreter::interpret(2, &domain);
    assert_ne!(a.preferred, b.preferred);
}

#[test]
fn preferred_is_permutation_of_domain() {
    let domain = domain();
    let bias = SeedInterpreter::interpret(0xDEAD_BEEF, &domain);
    let mut expected = domain.clone();
    expected.sort_by_key(|k| format!("{:?}", k));
    let mut actual = bias.preferred.clone();
    actual.sort_by_key(|k| format!("{:?}", k));
    assert_eq!(actual, expected);
}

#[test]
fn weights_cover_all_domain_kinds() {
    let domain = domain();
    let bias = SeedInterpreter::interpret(123, &domain);
    for kind in &domain {
        let key = format!("{:?}", kind);
        assert!(bias.weights.contains_key(&key));
    }
}

#[test]
fn weights_are_in_valid_range() {
    let domain = domain();
    let bias = SeedInterpreter::interpret(999, &domain);
    for (key, &w) in &bias.weights {
        assert!((1.0..2.0).contains(&w), "weight for {key} is {w}");
    }
}

#[test]
fn zero_seed_does_not_panic() {
    let bias = SeedInterpreter::interpret(0, &domain());
    assert_eq!(bias.preferred.len(), domain().len());
}

#[test]
fn empty_domain_produces_empty_bias() {
    let bias = SeedInterpreter::interpret(42, &[]);
    assert!(bias.preferred.is_empty());
    assert!(bias.weights.is_empty());
}
