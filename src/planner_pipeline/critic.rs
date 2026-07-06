use serde::Serialize;
use crate::planner_pipeline::Plan;

#[derive(Debug, Clone, Serialize)]
pub struct CriticReport {
    pub invariant_violations: Vec<String>,
    pub warnings: Vec<String>,
    pub passed: bool,
}

pub struct PlannerCritic;

impl PlannerCritic {
    pub fn analyze(&self, plan: &Plan) -> CriticReport {
        let mut violations = Vec::new();
        let mut warnings = Vec::new();

        if plan.id.is_empty() {
            violations.push("plan.id must not be empty".into());
        }
        if plan.steps.is_empty() {
            violations.push("plan.steps must not be empty".into());
        }
        if plan.seed == 0 {
            warnings.push("seed=0 is valid but unusual; verify intent".into());
        }

        // Check for duplicate steps
        let mut seen = std::collections::HashSet::new();
        for step in &plan.steps {
            if !seen.insert(step) {
                violations.push(format!("duplicate step detected: {:?}", step));
            }
        }

        // Check for empty individual steps
        for step in &plan.steps {
            if step.trim().is_empty() {
                violations.push("plan contains an empty step".into());
            }
        }

        let passed = violations.is_empty();
        CriticReport { invariant_violations: violations, warnings, passed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_plan() -> Plan {
        Plan {
            id: "abc123".into(),
            steps: vec!["step one".into(), "step two".into()],
            seed: 42,
        }
    }

    #[test]
    fn critic_passes_valid_plan() {
        let report = PlannerCritic.analyze(&valid_plan());
        assert!(report.passed);
        assert!(report.invariant_violations.is_empty());
    }

    #[test]
    fn critic_catches_empty_id() {
        let plan = Plan { id: "".into(), ..valid_plan() };
        let report = PlannerCritic.analyze(&plan);
        assert!(!report.passed);
        assert!(report.invariant_violations.iter().any(|v| v.contains("id")));
    }

    #[test]
    fn critic_catches_duplicate_steps() {
        let plan = Plan {
            steps: vec!["step one".into(), "step one".into()],
            ..valid_plan()
        };
        let report = PlannerCritic.analyze(&plan);
        assert!(!report.passed);
        assert!(report.invariant_violations.iter().any(|v| v.contains("duplicate")));
    }

    #[test]
    fn critic_does_not_mutate_plan() {
        let plan = valid_plan();
        let steps_before = plan.steps.clone();
        PlannerCritic.analyze(&plan);
        assert_eq!(plan.steps, steps_before);
    }
}
