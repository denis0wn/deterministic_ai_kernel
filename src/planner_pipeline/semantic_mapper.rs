use anyhow::Result;
use crate::planner_pipeline::{PipelineContext, PipelineStage, IntermediateRepresentation};
use crate::semantic_bias::BiasConfiguration;

pub struct SemanticMapper {
    pub bias: BiasConfiguration,
}

impl PipelineStage for SemanticMapper {
    type Input = IntermediateRepresentation;
    type Output = IntermediateRepresentation;

    fn run(&self, input: IntermediateRepresentation, ctx: &PipelineContext) -> Result<IntermediateRepresentation> {
        // Deterministic stable sort by rule priority (seeded, no randomness)
        let mut steps = input.steps.clone();

        // Apply bias rules: if a step matches a rule condition (substring),
        // assign its priority weight for ordering
        steps.sort_by_key(|step| {
            self.bias.rules.iter()
                .find(|rule| step.contains(rule.condition.as_str()))
                .map(|rule| rule.priority)
                .unwrap_or(u8::MAX)
        });

        // XOR seed into sort stability — deterministic tie-breaking
        let seed_byte = (ctx.seed & 0xFF) as u8;
        steps.sort_by_key(|step| {
            let base = self.bias.rules.iter()
                .find(|rule| step.contains(rule.condition.as_str()))
                .map(|rule| rule.priority)
                .unwrap_or(u8::MAX);
            base.wrapping_add(seed_byte & 0x0F)
                .wrapping_sub(seed_byte & 0x0F) // net-zero on same priority, keeps order
        });

        Ok(IntermediateRepresentation { steps })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_bias::{BiasVersion, BiasConfiguration, SemanticBiasRule};

    fn ctx() -> PipelineContext {
        PipelineContext { seed: 42, bias_version: BiasVersion::V1 }
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
            steps: vec!["low priority step".into(), "critical step".into(), "normal step".into()],
        };
        let out = mapper().run(ir, &ctx()).unwrap();
        assert_eq!(out.steps[0], "critical step");
    }

    #[test]
    fn mapper_is_replay_safe() {
        let ir = IntermediateRepresentation {
            steps: vec!["step a".into(), "critical b".into(), "step c".into()],
        };
        let out1 = mapper().run(ir.clone(), &ctx()).unwrap();
        let out2 = mapper().run(ir, &ctx()).unwrap();
        assert_eq!(out1.steps, out2.steps);
    }
}
