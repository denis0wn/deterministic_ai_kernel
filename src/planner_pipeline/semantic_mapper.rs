use crate::planner_pipeline::{IntermediateRepresentation, PipelineContext, PipelineStage};
use crate::semantic_bias::BiasConfiguration;
use anyhow::Result;

pub struct SemanticMapper {
    pub bias: BiasConfiguration,
}

impl SemanticMapper {
    /// Compute a deterministic tie-break key from step content and seed.
    /// Same (step, seed) always produces the same key.
    /// Different steps or different seeds produce different keys.
    fn tie_break_key(step: &str, seed: u64) -> u64 {
        let mut input = Vec::with_capacity(step.len() + 8);
        input.extend_from_slice(step.as_bytes());
        input.extend_from_slice(&seed.to_le_bytes());
        let hash = blake3::hash(&input);
        u64::from_le_bytes(hash.as_bytes()[..8].try_into().unwrap())
    }
}

impl PipelineStage for SemanticMapper {
    type Input = IntermediateRepresentation;
    type Output = IntermediateRepresentation;

    fn run(
        &self,
        input: IntermediateRepresentation,
        ctx: &PipelineContext,
    ) -> Result<IntermediateRepresentation> {
        let mut steps = input.steps.clone();

        // Single sort: primary key = priority (lower = earlier),
        // secondary key = deterministic tie-break from (step content, seed).
        // Steps with different priorities are ordered by priority only.
        // Steps with the same priority are ordered by seeded hash.
        steps.sort_by(|a, b| {
            let pri_a = self
                .bias
                .rules
                .iter()
                .find(|rule| a.contains(rule.condition.as_str()))
                .map(|rule| rule.priority)
                .unwrap_or(u8::MAX);
            let pri_b = self
                .bias
                .rules
                .iter()
                .find(|rule| b.contains(rule.condition.as_str()))
                .map(|rule| rule.priority)
                .unwrap_or(u8::MAX);

            pri_a.cmp(&pri_b).then_with(|| {
                Self::tie_break_key(a, ctx.seed).cmp(&Self::tie_break_key(b, ctx.seed))
            })
        });

        Ok(IntermediateRepresentation { steps })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_bias::{BiasConfiguration, BiasVersion, SemanticBiasRule};

    fn ctx(seed: u64) -> PipelineContext {
        PipelineContext {
            seed,
            bias_version: BiasVersion::V1,
            task_id: None,
        }
    }

    fn mapper() -> SemanticMapper {
        SemanticMapper {
            bias: BiasConfiguration::new(
                "test",
                vec![
                    SemanticBiasRule::new("r1", 1, "critical", "first"),
                    SemanticBiasRule::new("r2", 5, "normal", "middle"),
                    SemanticBiasRule::new("r3", 10, "low", "last"),
                ],
            ),
        }
    }

    #[test]
    fn mapper_orders_by_priority() {
        let ir = IntermediateRepresentation {
            steps: vec![
                "low priority step".into(),
                "critical step".into(),
                "normal step".into(),
            ],
        };
        let out = mapper().run(ir, &ctx(42)).expect("test failure");
        assert_eq!(out.steps[0], "critical step");
        assert_eq!(out.steps[1], "normal step");
        assert_eq!(out.steps[2], "low priority step");
    }

    #[test]
    fn mapper_is_replay_safe() {
        let ir = IntermediateRepresentation {
            steps: vec!["step a".into(), "critical b".into(), "step c".into()],
        };
        let out1 = mapper().run(ir.clone(), &ctx(42)).expect("test failure");
        let out2 = mapper().run(ir, &ctx(42)).expect("test failure");
        assert_eq!(out1.steps, out2.steps);
    }

    #[test]
    fn different_seeds_change_same_priority_order() {
        // Two steps with same priority (no rule match => u8::MAX)
        let ir = IntermediateRepresentation {
            steps: vec!["alpha step".into(), "beta step".into()],
        };
        let out1 = mapper().run(ir.clone(), &ctx(1)).expect("test failure");
        let out2 = mapper().run(ir, &ctx(99)).expect("test failure");
        // Same priority but different seed => different tie-break => different order
        assert_ne!(out1.steps, out2.steps);
    }

    #[test]
    fn different_priorities_dominate_seed() {
        // "critical" matches rule with priority 1, "low" matches priority 10
        // Regardless of seed, critical should come first
        let ir = IntermediateRepresentation {
            steps: vec!["low step".into(), "critical step".into()],
        };
        for seed in [1u64, 42, 99, 255] {
            let out = mapper().run(ir.clone(), &ctx(seed)).expect("test failure");
            assert_eq!(out.steps[0], "critical step", "failed for seed={seed}");
        }
    }

    #[test]
    fn tie_break_key_is_deterministic() {
        let k1 = SemanticMapper::tie_break_key("hello", 42);
        let k2 = SemanticMapper::tie_break_key("hello", 42);
        assert_eq!(k1, k2);
    }

    #[test]
    fn tie_break_key_differs_for_different_steps() {
        let k1 = SemanticMapper::tie_break_key("step a", 42);
        let k2 = SemanticMapper::tie_break_key("step b", 42);
        assert_ne!(k1, k2);
    }

    #[test]
    fn tie_break_key_differs_for_different_seeds() {
        let k1 = SemanticMapper::tie_break_key("step a", 1);
        let k2 = SemanticMapper::tie_break_key("step a", 99);
        assert_ne!(k1, k2);
    }

    #[test]
    fn empty_steps_ok() {
        let ir = IntermediateRepresentation { steps: vec![] };
        let out = mapper().run(ir, &ctx(42)).expect("test failure");
        assert!(out.steps.is_empty());
    }

    #[test]
    fn no_matching_rules_uses_max_priority() {
        let ir = IntermediateRepresentation {
            steps: vec!["unrelated step".into(), "another unrelated".into()],
        };
        // Both get u8::MAX, tie-broken by seeded hash
        let out = mapper().run(ir, &ctx(42)).expect("test failure");
        assert_eq!(out.steps.len(), 2);
    }
}
