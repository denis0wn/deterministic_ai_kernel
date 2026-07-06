use anyhow::{bail, Result};
use crate::planner_pipeline::{PipelineContext, PipelineStage, RawInput, IntermediateRepresentation};

pub struct Parser;

impl Parser {
    fn extract_steps(payload: &str) -> Vec<String> {
        let lines: Vec<&str> = payload.lines().collect();

        // Numbered list: "1. step" or "1) step"
        let numbered: Vec<String> = lines.iter()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.splitn(2, |c: char| c == '.' || c == ')')
                    .collect::<Vec<_>>();
                if rest.len() == 2 && rest[0].chars().all(|c| c.is_ascii_digit()) && !rest[0].is_empty() {
                    let step = rest[1].trim().to_string();
                    if !step.is_empty() { Some(step) } else { None }
                } else {
                    None
                }
            })
            .collect();

        if !numbered.is_empty() {
            return numbered;
        }

        // Bullet list: "- step", "* step", "• step"
        let bulleted: Vec<String> = lines.iter()
            .filter_map(|l| {
                let l = l.trim();
                if let Some(rest) = l.strip_prefix("- ")
                    .or_else(|| l.strip_prefix("* "))
                    .or_else(|| l.strip_prefix("• "))
                    .or_else(|| l.strip_prefix("– "))
                    .or_else(|| l.strip_prefix("— "))
                {
                    let step = rest.trim().to_string();
                    if !step.is_empty() { Some(step) } else { None }
                } else {
                    None
                }
            })
            .collect();

        if !bulleted.is_empty() {
            return bulleted;
        }

        // Paragraph split: blank lines separate steps
        let paragraphs: Vec<String> = payload
            .split("\n\n")
            .map(|p| p.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "))
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();

        if paragraphs.len() > 1 {
            return paragraphs;
        }

        // Fallback: non-empty lines
        lines.iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }
}

impl PipelineStage for Parser {
    type Input = RawInput;
    type Output = IntermediateRepresentation;

    fn run(&self, input: RawInput, _ctx: &PipelineContext) -> Result<IntermediateRepresentation> {
        let steps = Self::extract_steps(&input.payload);

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
    fn parser_numbered_list() {
        let input = RawInput { payload: "1. Реализовать API\n2. Написать тесты\n3. Задокументировать".into() };
        let ir = Parser.run(input, &ctx()).unwrap();
        assert_eq!(ir.steps, vec!["Реализовать API", "Написать тесты", "Задокументировать"]);
    }

    #[test]
    fn parser_bullet_dash() {
        let input = RawInput { payload: "- Шаг один\n- Шаг два\n- Шаг три".into() };
        let ir = Parser.run(input, &ctx()).unwrap();
        assert_eq!(ir.steps, vec!["Шаг один", "Шаг два", "Шаг три"]);
    }

    #[test]
    fn parser_bullet_star() {
        let input = RawInput { payload: "* Шаг один\n* Шаг два".into() };
        let ir = Parser.run(input, &ctx()).unwrap();
        assert_eq!(ir.steps, vec!["Шаг один", "Шаг два"]);
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

    #[test]
    fn parser_strips_numbering() {
        let input = RawInput { payload: "1) First\n2) Second".into() };
        let ir = Parser.run(input, &ctx()).unwrap();
        assert_eq!(ir.steps, vec!["First", "Second"]);
    }
}
