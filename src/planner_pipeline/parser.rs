use crate::planner_pipeline::{
    IntermediateRepresentation, PipelineContext, PipelineStage, RawInput,
};
use anyhow::{bail, Result};

pub struct Parser;

/// Coordination conjunctions that signal multiple actions in free-form text.
const CONJUNCTIONS: &[&str] = &[
    " and ",
    " then ",
    " also ",
    " additionally ",
    " furthermore ",
    " and then ",
    " after that ",
    " и ",
    " затем ",
    " также ",
    " после этого ",
    " и также ",
];

/// Minimum payload length to consider for decomposition.
const DECOMPOSE_LENGTH_THRESHOLD: usize = 60;

/// Action verbs (English) — words that signal an actionable step.
const ACTION_VERBS_EN: &[&str] = &[
    "refactor",
    "add",
    "update",
    "analyze",
    "propose",
    "read",
    "find",
    "patch",
    "audit",
    "classify",
    "recommend",
    "fix",
    "write",
    "create",
    "remove",
    "test",
    "document",
    "implement",
    "debug",
    "optimize",
    "review",
    "deploy",
    "build",
    "configure",
    "migrate",
    "validate",
    "verify",
    "extract",
    "rename",
    "move",
    "split",
    "merge",
    "delete",
    "insert",
    "replace",
    "check",
    "measure",
    "profile",
    "benchmark",
    "compare",
    "evaluate",
    "design",
    "plan",
    "estimate",
    "calculate",
    "compute",
    "parse",
    "normalize",
    "transform",
    "convert",
    "encode",
    "decode",
    "encrypt",
    "decrypt",
    "compress",
    "decompress",
    "upload",
    "download",
    "sync",
];

/// Action verbs (Russian) — words that signal an actionable step.
const ACTION_VERBS_RU: &[&str] = &[
    "сделай",
    "добавь",
    "обнови",
    "проверь",
    "найди",
    "исправь",
    "удали",
    "напиши",
    "создай",
    "реализуй",
    "настрой",
    "мигрируй",
    "валидируй",
    "отладь",
    "оптимизируй",
    "проведи",
    "разверни",
    "собери",
    "нарисуй",
    "проанализируй",
    "предложи",
    "прочитай",
    "замени",
    "переименуй",
    "перемести",
    "раздели",
    "объедини",
    "вставь",
    "запусти",
    "останови",
    "измени",
    "измерь",
    "сравни",
    "оцени",
    "спроектируй",
];

/// Minimum length for a chunk to be considered a valid step after action-chunk split.
const ACTION_CHUNK_MIN_LEN: usize = 15;

/// Maximum number of steps we'll produce from action-chunk detection.
const ACTION_CHUNK_MAX_STEPS: usize = 8;

impl Parser {
    /// Check if a text starts with an action verb (after trimming).
    fn starts_with_action_verb(chunk: &str) -> bool {
        let lower = chunk.trim().to_lowercase();
        ACTION_VERBS_EN.iter().any(|v| lower.starts_with(v))
            || ACTION_VERBS_RU.iter().any(|v| lower.starts_with(v))
    }

    /// Split text by comma+conjunction boundaries, producing raw chunks.
    /// Splits on ", " and conjunctions, but keeps "and X" attached to the previous chunk
    /// when the next chunk doesn't start with an action verb.
    fn split_action_chunks(text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        let mut chunks = Vec::new();

        // First split by comma
        for part in text.split(',') {
            let part = part.trim().to_string();
            if !part.is_empty() {
                chunks.push(part);
            }
        }

        // If comma split produced only 1 chunk, try conjunction split
        if chunks.len() <= 1 {
            chunks.clear();
            for conj in CONJUNCTIONS {
                if lower.contains(conj) {
                    for part in lower.split(conj) {
                        let part = part.trim().to_string();
                        if !part.is_empty() {
                            chunks.push(part);
                        }
                    }
                    break; // use first matching conjunction
                }
            }
        }

        // Post-process: merge trailing fragments that don't start with action verbs
        // into the previous chunk (e.g., "and tests" → merge into previous)
        let mut merged: Vec<String> = Vec::new();
        for chunk in chunks {
            let chunk = chunk.trim().trim_end_matches('.').trim().to_string();
            if chunk.is_empty() {
                continue;
            }

            if let Some(last) = merged.last_mut() {
                // If this chunk doesn't start with an action verb, it's likely a continuation
                if !Self::starts_with_action_verb(&chunk) && last.len() + chunk.len() < 200 {
                    last.push_str(", ");
                    last.push_str(&chunk);
                    continue;
                }
            }
            merged.push(chunk);
        }

        merged
    }

    /// Decompose a single-step free-form task into multiple actionable steps.
    /// Returns None if the task doesn't need decomposition.
    pub(crate) fn decompose_free_form(text: &str) -> Option<Vec<String>> {
        let trimmed = text.trim();

        // Don't decompose short tasks
        if trimmed.len() < DECOMPOSE_LENGTH_THRESHOLD {
            return None;
        }

        // Level 1: split by semicolons (highest confidence, no conjunction needed)
        let parts = Self::split_by_semicolons(trimmed);
        if parts.len() > 1 {
            return Some(parts);
        }

        // Level 2: split by coordination conjunctions (need conjunctions present)
        if Self::has_conjunctions(trimmed) {
            let conj_parts = Self::split_by_conjunctions(trimmed);
            if conj_parts.len() > 1 {
                return Some(conj_parts);
            }
        }

        // Level 3: action-chunk detection — split by comma/conjunction,
        // validate each chunk starts with an action verb
        let action_chunks = Self::split_action_chunks(trimmed);
        let valid_chunks: Vec<String> = action_chunks
            .iter()
            .filter(|c| c.len() >= ACTION_CHUNK_MIN_LEN)
            .cloned()
            .collect();

        if valid_chunks.len() >= 2 && valid_chunks.len() <= ACTION_CHUNK_MAX_STEPS {
            // Verify at least half the chunks start with action verbs
            let verb_count = valid_chunks
                .iter()
                .filter(|c| Self::starts_with_action_verb(c))
                .count();
            if verb_count >= (valid_chunks.len() + 1) / 2 {
                return Some(valid_chunks);
            }
        }

        None
    }

    /// Check if a text contains coordination conjunctions between action phrases.
    fn has_conjunctions(text: &str) -> bool {
        let lower = text.to_lowercase();
        CONJUNCTIONS.iter().any(|c| lower.contains(c))
    }

    /// Split text by semicolons as a first-pass decomposition.
    fn split_by_semicolons(text: &str) -> Vec<String> {
        text.split(';')
            .map(|s| s.trim().trim_end_matches('.').trim().to_string())
            .filter(|s| !s.is_empty() && s.len() > 5)
            .collect()
    }

    /// Count how many action verbs appear in the text (case-insensitive).
    fn count_action_verbs(text: &str) -> usize {
        let lower = text.to_lowercase();
        let mut count = 0;
        for verb in ACTION_VERBS_EN.iter().chain(ACTION_VERBS_RU.iter()) {
            if lower.contains(verb) {
                count += 1;
            }
        }
        count
    }

    /// Split text by coordination conjunctions ("and", "then", "also", "и", "затем", "также").
    /// Only splits when each resulting part is a meaningful action (>10 chars).
    fn split_by_conjunctions(text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        let mut best_parts: Vec<String> = Vec::new();

        // Try each conjunction pattern, pick the one that produces the most meaningful parts
        for conj in CONJUNCTIONS {
            if !lower.contains(conj) {
                continue;
            }
            // Find the actual position of the conjunction in the original text
            let conj_lower = conj.trim();
            let parts: Vec<String> = text
                .split(|c: char| {
                    let _cl = c.to_lowercase().next().unwrap_or(c);
                    // Check if this char starts a conjunction match
                    let remaining = &text[text.find(c).unwrap_or(0)..];
                    remaining.to_lowercase().starts_with(conj_lower)
                })
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s.len() > 10)
                .collect();

            if parts.len() > best_parts.len() {
                best_parts = parts;
            }
        }

        best_parts
    }

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
        let fallback: Vec<String> = lines
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        // Decomposition: if we got exactly 1 step from a long free-form payload,
        // try to decompose it into actionable sub-steps
        if fallback.len() == 1 {
            if let Some(decomposed) = Self::decompose_free_form(&fallback[0]) {
                return decomposed;
            }
        }

        fallback
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

        // Observability: log decomposition result
        let action_count = Self::count_action_verbs(&input.payload);
        let trigger = if steps.len() > 1 {
            // Determine which strategy produced the steps
            if Self::split_by_semicolons(&input.payload).len() > 1 {
                "semicolon"
            } else if Self::has_conjunctions(&input.payload)
                && Self::split_by_conjunctions(&input.payload).len() > 1
            {
                "conjunction"
            } else if Self::split_action_chunks(&input.payload).len() > 1 {
                "action_chunk"
            } else {
                "explicit"
            }
        } else {
            "none"
        };

        eprintln!(
            "observability: component=parser operation=decompose \
             payload_len={} action_count={} steps={} trigger={}",
            input.payload.len(),
            action_count,
            steps.len(),
            trigger,
        );

        // Warn if long payload with multiple actions produced only 1 step
        if steps.len() == 1 && input.payload.len() > 150 && action_count >= 2 {
            eprintln!(
                "observability: component=parser operation=decomposition_insufficient \
                 payload_len={} action_count={} steps=1",
                input.payload.len(),
                action_count,
            );
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

    // ── Decomposition tests ──────────────────────────────────────────────────

    #[test]
    fn decomposition_short_task_stays_single() {
        // Short free-form task without conjunctions → no decomposition
        let input = RawInput {
            payload: "Fix the typo in README.md".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 1);
    }

    #[test]
    fn decomposition_multi_action_with_conjunctions() {
        // Long free-form task with "and" conjunctions → should decompose
        let input = RawInput {
            payload: "Refactor the parser module to support new syntax and add comprehensive tests for all edge cases and update the documentation to reflect the changes and verify backward compatibility".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() > 1,
            "Expected decomposition for multi-action task, got {} steps: {:?}",
            ir.steps.len(),
            ir.steps
        );
    }

    #[test]
    fn decomposition_deterministic() {
        // Same payload → same result
        let payload = "Refactor the parser module to support new syntax and add comprehensive tests for all edge cases and update the documentation to reflect the changes and verify backward compatibility";
        let input1 = RawInput {
            payload: payload.into(),
        };
        let input2 = RawInput {
            payload: payload.into(),
        };
        let ir1 = Parser.run(input1, &ctx()).expect("test failure");
        let ir2 = Parser.run(input2, &ctx()).expect("test failure");
        assert_eq!(ir1.steps, ir2.steps);
    }

    #[test]
    fn decomposition_russian_conjunctions() {
        // Russian conjunctions: "и", "затем", "также"
        let input = RawInput {
            payload: "Рефактори модуль парсера чтобы он поддерживал новый синтаксис и добавь тесты для всех edge cases и обнови документацию и проверь backward compatibility".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() > 1,
            "Expected decomposition for Russian multi-action, got {} steps",
            ir.steps.len()
        );
    }

    #[test]
    fn decomposition_semicolon_split() {
        // Semicolons are high-confidence split points
        let input = RawInput {
            payload: "Refactor the parser module; add comprehensive tests for all edge cases; update the documentation to reflect the changes; verify backward compatibility with existing tests".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() >= 3,
            "Expected ≥3 steps from semicolons, got {}",
            ir.steps.len()
        );
    }

    #[test]
    fn decomposition_empty_payload_errors() {
        let input = RawInput {
            payload: "   ".into(),
        };
        assert!(Parser.run(input, &ctx()).is_err());
    }

    #[test]
    fn decomposition_single_word_stays_single() {
        let input = RawInput {
            payload: "debug".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 1);
    }

    #[test]
    fn decomposition_abbreviation_not_split() {
        // "e.g." and "3.14" should not create false split points
        let input = RawInput {
            payload: "Use e.g. regex patterns like 3.14 for validation".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 1);
    }

    #[test]
    fn decomposition_no_over_split() {
        // A coherent single action with "and" but no second verb group
        // should NOT be split
        let input = RawInput {
            payload: "Fix the red and blue buttons in the header".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        // This is a single action, should stay as 1 step
        assert_eq!(ir.steps.len(), 1);
    }

    #[test]
    fn explicit_formats_unaffected() {
        // Explicit formats should still work after decomposition is added
        let input = RawInput {
            payload: "1. First step\n2. Second step\n3. Third step".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 3);
    }

    #[test]
    fn bullet_format_unaffected() {
        let input = RawInput {
            payload: "- Step one\n- Step two\n- Step three".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 3);
    }

    // ── Action-chunk detection tests ─────────────────────────────────────────

    #[test]
    fn action_chunk_refactor_add_update() {
        let input = RawInput {
            payload: "Refactor file_tools into smaller modules, add tests, and update docs".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() >= 2,
            "Expected ≥2 steps from action-chunk, got {} steps: {:?}",
            ir.steps.len(),
            ir.steps
        );
    }

    #[test]
    fn action_chunk_analyze_propose() {
        let input = RawInput {
            payload: "Analyze the codebase and propose fixes for performance issues".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() >= 2,
            "Expected ≥2 steps, got {} steps: {:?}",
            ir.steps.len(),
            ir.steps
        );
    }

    #[test]
    fn action_chunk_audit_classify_recommend() {
        let input = RawInput {
            payload: "Audit the auth module, classify vulnerabilities, and recommend patches for each critical finding".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() >= 2,
            "Expected ≥2 steps, got {} steps: {:?}",
            ir.steps.len(),
            ir.steps
        );
    }

    #[test]
    fn action_chunk_read_find_patch() {
        let input = RawInput {
            payload: "Read src/tools/contract.rs, find the bug in error handling, and patch it with proper Result propagation".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() >= 2,
            "Expected ≥2 steps, got {} steps: {:?}",
            ir.steps.len(),
            ir.steps
        );
    }

    #[test]
    fn action_chunk_russian_verbs() {
        let input = RawInput {
            payload: "Проанализируй модуль парсера, предложи оптимизации, и напиши тесты для новых кейсов".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert!(
            ir.steps.len() >= 2,
            "Expected ≥2 steps from Russian action-chunks, got {} steps: {:?}",
            ir.steps.len(),
            ir.steps
        );
    }

    #[test]
    fn decomposition_still_short_stays_single() {
        let input = RawInput {
            payload: "Fix typo in README".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 1);
    }

    #[test]
    fn decomposition_explicit_numbered_unaffected() {
        let input = RawInput {
            payload: "1. First\n2. Second\n3. Third".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 3);
    }

    #[test]
    fn decomposition_deterministic_action_chunk() {
        let payload = "Refactor file_tools into smaller modules, add tests, and update docs";
        let ir1 = Parser
            .run(
                RawInput {
                    payload: payload.into(),
                },
                &ctx(),
            )
            .expect("test failure");
        let ir2 = Parser
            .run(
                RawInput {
                    payload: payload.into(),
                },
                &ctx(),
            )
            .expect("test failure");
        assert_eq!(ir1.steps, ir2.steps);
    }

    #[test]
    fn decomposition_no_split_single_action() {
        // "Fix the red and blue buttons" is ONE action, should not split
        let input = RawInput {
            payload: "Fix the red and blue buttons in the header of the main page".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 1);
    }

    #[test]
    fn action_chunk_too_few_verbs_stays_single() {
        // "Refactor the parser module to support new syntax" — single action, no split
        let input = RawInput {
            payload: "Refactor the parser module to support new syntax".into(),
        };
        let ir = Parser.run(input, &ctx()).expect("test failure");
        assert_eq!(ir.steps.len(), 1);
    }
}
