use anyhow::{bail, Result};
use crate::planner_pipeline::{PipelineContext, PipelineStage, RawInput, IntermediateRepresentation};

pub struct Parser;

impl PipelineStage for Parser {
    type Input = RawInput;
    type Output = IntermediateRepresentation;

    fn run(&self, input: RawInput, _ctx: &PipelineContext) -> Result<IntermediateRepresentation> {
        let steps: Vec<String> = input.payload
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        if steps.is_empty() {
            bail!("Parser: payload produced no steps after trimming");
        }

        Ok(IntermediateRepresentation { steps })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_bias::BiasVersion;

    fn ctx() -> PipelineContext {
        PipelineContext { seed: 42, bias_version: BiasVersion::V1 }
    }

    #[test]
    fn parser_splits_lines_into_steps() {
        let input = RawInput { payload: "step one\nstep two\nstep three".into() };
        let ir = Parser.run(input, &ctx()).unwrap();
        assert_eq!(ir.steps, vec!["step one", "step two", "step three"]);
    }

    #[test]
    fn parser_skips_empty_lines() {
        let input = RawInput { payload: "step one\n\n  \nstep two".into() };
        let ir = Parser.run(input, &ctx()).unwrap();
        assert_eq!(ir.steps.len(), 2);
    }

    #[test]
    fn parser_errors_on_empty_payload() {
        let input = RawInput { payload: "   \n  \n".into() };
        assert!(Parser.run(input, &ctx()).is_err());
    }
}
