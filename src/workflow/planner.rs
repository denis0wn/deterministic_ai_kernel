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

    let has = |needle: &str| tokens.iter().any(|t| *t == needle);
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
    use super::{normalize_step, parse_steps, validate_steps, Step, StepKind};

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
}
