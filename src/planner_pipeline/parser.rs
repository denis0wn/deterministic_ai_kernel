use crate::planner_pipeline::{
    IntermediateRepresentation, PipelineContext, PipelineStage, RawInput,
};
use anyhow::{bail, Result};

pub struct Parser;

impl Parser {
    fn extract_explicit_steps(payload: &str) -> Vec<String> {
        let mut steps = Vec::new();

        // 1. Try line-by-line matching
        for line in payload.lines() {
            let line = line.trim();
            let line_lower = line.to_lowercase();
            if line_lower.starts_with("step ") {
                let rest = &line[5..];
                let char_indices = rest.char_indices();
                let mut digit_len = 0;
                for (_, c) in char_indices {
                    if c.is_ascii_digit() {
                        digit_len += 1;
                    } else {
                        break;
                    }
                }
                if digit_len > 0 && rest[digit_len..].starts_with(' ') {
                    let mut step_text = rest[digit_len + 1..].trim().to_string();
                    if step_text.ends_with('.') {
                        step_text.pop();
                    }
                    step_text = step_text.trim().to_string();
                    if !step_text.is_empty() {
                        steps.push(step_text);
                    }
                }
            }
        }

        if !steps.is_empty() {
            return steps;
        }

        // 2. Try inline scanning (single-line or paragraph formats)
        let chars: Vec<char> = payload.chars().collect();
        let mut i = 0;
        let mut start_indices = Vec::new();

        while i < chars.len() {
            if i + 5 <= chars.len()
                && (chars[i] == 's' || chars[i] == 'S')
                && (chars[i + 1] == 't' || chars[i + 1] == 'T')
                && (chars[i + 2] == 'e' || chars[i + 2] == 'E')
                && (chars[i + 3] == 'p' || chars[i + 3] == 'P')
                && chars[i + 4] == ' '
            {
                let mut j = i + 5;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 5 && j < chars.len() && chars[j] == ' ' {
                    start_indices.push((i, j + 1));
                    i = j;
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        for idx in 0..start_indices.len() {
            let start_char = start_indices[idx].1;
            let end_char = if idx + 1 < start_indices.len() {
                start_indices[idx + 1].0
            } else {
                chars.len()
            };
            let mut step_text: String = chars[start_char..end_char].iter().collect();
            step_text = step_text.trim().to_string();
            if step_text.ends_with('.') {
                step_text.pop();
            }
            step_text = step_text.trim().to_string();
            if !step_text.is_empty() {
                steps.push(step_text);
            }
        }

        steps
    }

    fn extract_steps(payload: &str) -> Vec<String> {
        let explicit = Self::extract_explicit_steps(payload);
        if !explicit.is_empty() {
            return explicit;
        }

        let lines: Vec<&str> = payload.lines().collect();

        // Numbered list: "1. step" or "1) step"
        let numbered: Vec<String> = lines
            .iter()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.splitn(2, ['.', ')']).collect::<Vec<_>>();
                if rest.len() == 2
                    && rest[0].chars().all(|c| c.is_ascii_digit())
                    && !rest[0].is_empty()
                {
                    let step = rest[1].trim().to_string();
                    if !step.is_empty() {
                        Some(step)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if !numbered.is_empty() {
            return numbered;
        }

        // Bullet list: "- step", "* step", "• step"
        let bulleted: Vec<String> = lines
            .iter()
            .filter_map(|l| {
                let l = l.trim();
                if let Some(rest) = l
                    .strip_prefix("- ")
                    .or_else(|| l.strip_prefix("* "))
                    .or_else(|| l.strip_prefix("• "))
                    .or_else(|| l.strip_prefix("– "))
                    .or_else(|| l.strip_prefix("— "))
                {
                    let step = rest.trim().to_string();
                    if !step.is_empty() {
                        Some(step)
                    } else {
                        None
                    }
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
            .map(|p| {
                p.lines()
                    .map(|l| l.trim())
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();

        if paragraphs.len() > 1 {
            return paragraphs;
        }

        // Fallback: non-empty lines
        lines
            .iter()
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
        PipelineContext {
            seed: 42,
            bias_version: BiasVersion::V1,
            task_id: None,
        }
    }

    #[test]
    fn parser_numbered_list() {
        let input = RawInput {
            payload: "1. Реализовать API\n2. Написать тесты\n3. Задокументировать".into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse numbered list");
        assert_eq!(
            ir.steps,
            vec!["Реализовать API", "Написать тесты", "Задокументировать"]
        );
    }

    #[test]
    fn parser_bullet_dash() {
        let input = RawInput {
            payload: "- Шаг один\n- Шаг два\n- Шаг три".into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse bullet list with dash");
        assert_eq!(ir.steps, vec!["Шаг один", "Шаг два", "Шаг три"]);
    }

    #[test]
    fn parser_bullet_star() {
        let input = RawInput {
            payload: "* Шаг один\n* Шаг два".into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse bullet list with star");
        assert_eq!(ir.steps, vec!["Шаг один", "Шаг два"]);
    }

    #[test]
    fn parser_splits_lines_into_steps() {
        let input = RawInput {
            payload: "step one\nstep two\nstep three".into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse multi-line steps");
        assert_eq!(ir.steps, vec!["step one", "step two", "step three"]);
    }

    #[test]
    fn parser_skips_empty_lines() {
        let input = RawInput {
            payload: "step one\n\n  \nstep two".into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse steps with empty lines");
        assert_eq!(ir.steps.len(), 2);
    }

    #[test]
    fn parser_errors_on_empty_payload() {
        let input = RawInput {
            payload: "   \n  \n".into(),
        };
        assert!(Parser.run(input, &ctx()).is_err());
    }

    #[test]
    fn parser_strips_numbering() {
        let input = RawInput {
            payload: "1) First\n2) Second".into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse parenthesized numbering");
        assert_eq!(ir.steps, vec!["First", "Second"]);
    }

    #[test]
    fn parser_explicit_steps_inline() {
        let input = RawInput {
            payload: "Customer request: build website. Step 1 create design. Step 2 implement backend. Step 3 run tests. Step 4 deploy.".into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse inline explicit steps");
        assert_eq!(
            ir.steps,
            vec!["create design", "implement backend", "run tests", "deploy"]
        );
    }

    #[test]
    fn parser_explicit_steps_multiline() {
        let input = RawInput {
            payload: "Step 1 prepare database\nStep 2 create API\nStep 3 run integration tests"
                .into(),
        };
        let ir = Parser
            .run(input, &ctx())
            .expect("failed to parse multi-line explicit steps");
        assert_eq!(
            ir.steps,
            vec!["prepare database", "create API", "run integration tests"]
        );
    }
}
