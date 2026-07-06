use crate::planner_pipeline::{PipelineContext, PipelineStage, RawInput};
use anyhow::Result;

pub struct Normalizer;

impl PipelineStage for Normalizer {
    type Input = RawInput;
    type Output = RawInput;

    fn run(&self, input: RawInput, _ctx: &PipelineContext) -> Result<RawInput> {
        // Normalize each line independently, preserving line boundaries for Parser
        let normalized = input.payload
            .lines()
            .map(|line| {
                line.chars()
                    .filter(|&c| !matches!(c, '\x00'..='\x08' | '\x0B'..='\x0C' | '\x0E'..='\x1F' | '\x7F'))
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase()
            })
            .collect::<Vec<_>>()
            .join("\n");

        Ok(RawInput {
            payload: normalized,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_bias::BiasVersion;

    fn ctx() -> PipelineContext {
        PipelineContext {
            seed: 42,
            bias_version: BiasVersion::V1,
        }
    }

    #[test]
    fn normalizer_collapses_whitespace() {
        let input = RawInput {
            payload: "  Step  ONE  \n  Step TWO  ".into(),
        };
        let out = Normalizer.run(input, &ctx()).unwrap();
        assert_eq!(out.payload, "step one\nstep two");
    }

    #[test]
    fn normalizer_preserves_line_boundaries() {
        let input = RawInput {
            payload: "line one\nline two\nline three".into(),
        };
        let out = Normalizer.run(input, &ctx()).unwrap();
        assert_eq!(out.payload.lines().count(), 3);
    }

    #[test]
    fn normalizer_is_idempotent() {
        let input = RawInput {
            payload: "  Hello   World \t\n  Foo  BAR  ".into(),
        };
        let first = Normalizer.run(input, &ctx()).unwrap();
        let second = Normalizer
            .run(
                RawInput {
                    payload: first.payload.clone(),
                },
                &ctx(),
            )
            .unwrap();
        assert_eq!(first.payload, second.payload);
    }

    #[test]
    fn normalizer_strips_control_chars() {
        let input = RawInput {
            payload: "step\x01one\x7Ftwo".into(),
        };
        let out = Normalizer.run(input, &ctx()).unwrap();
        assert!(!out.payload.contains('\x01'));
        assert!(!out.payload.contains('\x7F'));
    }

    #[test]
    fn normalizer_lowercases() {
        let input = RawInput {
            payload: "UPPER lower MiXeD".into(),
        };
        let out = Normalizer.run(input, &ctx()).unwrap();
        assert_eq!(out.payload, "upper lower mixed");
    }
}
