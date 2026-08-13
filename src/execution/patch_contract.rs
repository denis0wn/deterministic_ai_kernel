//! Structured CodeFix patch contract (P1, H-1 fix).
//!
//! The kernel owns the patch as typed data. Free-form LLM text is untrusted
//! input: it must parse into `PatchV1` and pass kernel-side validation
//! before anything else may consume it. Shape or grounding failures are
//! contract violations, reported with a `fatal:` prefix so the failure
//! classifier treats them as terminal (audit: "the LLM must never directly
//! declare task success").

use serde::{Deserialize, Serialize};

pub const PATCH_CONTRACT_VERSION: &str = "patch_v1";

/// Declared validation for a patch (P1 records it; P3 executes it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchValidationSpec {
    pub command: String,
    #[serde(default)]
    pub expected_exit: i32,
}

/// The only patch representation the kernel accepts from a model.
///
/// `context_before` must be an exact byte-for-byte substring of the target
/// file and must occur exactly once; `replacement` must differ from it
/// (no-op patches are rejected).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchV1 {
    pub version: String,
    pub target_file: String,
    pub context_before: String,
    pub replacement: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub validation: Option<PatchValidationSpec>,
}

#[derive(Debug, PartialEq)]
pub enum PatchShapeError {
    WrongVersion(String),
    EmptyField(&'static str),
    NoOpPatch,
    PathTraversal(String),
}

impl std::fmt::Display for PatchShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchShapeError::WrongVersion(v) => {
                write!(
                    f,
                    "unsupported patch version '{v}' (expected {PATCH_CONTRACT_VERSION})"
                )
            }
            PatchShapeError::EmptyField(name) => write!(f, "field '{name}' is empty"),
            PatchShapeError::NoOpPatch => {
                write!(f, "no-op patch rejected: replacement equals context_before")
            }
            PatchShapeError::PathTraversal(p) => {
                write!(f, "path traversal rejected in target_file '{p}'")
            }
        }
    }
}

/// Kernel-side shape validation. Pure: no filesystem, no LLM.
pub fn validate_patch_shape(patch: &PatchV1) -> Result<(), PatchShapeError> {
    if patch.version != PATCH_CONTRACT_VERSION {
        return Err(PatchShapeError::WrongVersion(patch.version.clone()));
    }
    if patch.target_file.trim().is_empty() {
        return Err(PatchShapeError::EmptyField("target_file"));
    }
    if patch.context_before.is_empty() {
        return Err(PatchShapeError::EmptyField("context_before"));
    }
    if patch.reason.trim().is_empty() {
        return Err(PatchShapeError::EmptyField("reason"));
    }
    if patch.context_before == patch.replacement {
        return Err(PatchShapeError::NoOpPatch);
    }
    let traversal = std::path::Path::new(&patch.target_file)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if traversal {
        return Err(PatchShapeError::PathTraversal(patch.target_file.clone()));
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
pub enum PatchGroundingError {
    ContextNotFound,
    ContextAmbiguous(usize),
}

impl std::fmt::Display for PatchGroundingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchGroundingError::ContextNotFound => write!(
                f,
                "context_before not found in target file (model hallucinated file content)"
            ),
            PatchGroundingError::ContextAmbiguous(n) => write!(
                f,
                "context_before occurs {n} times in target file; it must be unique"
            ),
        }
    }
}

/// Grounding validation against real file content: the proposed context must
/// exist exactly once. Returns the occurrence count (always 1 on Ok).
pub fn validate_patch_against_content(
    patch: &PatchV1,
    file_content: &str,
) -> Result<usize, PatchGroundingError> {
    match file_content.matches(&patch.context_before).count() {
        0 => Err(PatchGroundingError::ContextNotFound),
        1 => Ok(1),
        n => Err(PatchGroundingError::ContextAmbiguous(n)),
    }
}

/// Parse untrusted model text into a typed patch. Accepts raw JSON or JSON
/// embedded in prose/fences (via llm::extract_json). Any deviation is an
/// error — the caller decides about one bounded repair retry.
pub fn parse_patch(text: &str) -> Result<PatchV1, String> {
    let candidate = crate::llm::extract_json(text);
    serde_json::from_str::<PatchV1>(&candidate)
        .map_err(|e| format!("patch_v1 schema violation: {e}"))
}

const CODE_FILE_EXTENSIONS: &[&str] = &[
    ".rs", ".py", ".js", ".ts", ".go", ".c", ".h", ".cpp", ".hpp", ".java", ".rb", ".sh", ".swift",
    ".toml", ".json", ".sql", ".html", ".css", ".md", ".txt",
];

/// Deterministic target-file extraction from free task text.
///
/// Prefers path-shaped tokens (containing '/'), then tokens ending in a
/// known code extension. URLs are excluded. Pure heuristic — the kernel
/// still verifies existence and content before trusting anything.
pub fn extract_target_file(text: &str) -> Option<String> {
    fn clean(token: &str) -> &str {
        token.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '/' | '.' | '-')))
    }

    for token in text.split_whitespace() {
        let candidate = clean(token);
        if candidate.contains('/') && !candidate.starts_with("http") && candidate.len() > 1 {
            return Some(candidate.to_string());
        }
    }
    for token in text.split_whitespace() {
        let candidate = clean(token);
        if CODE_FILE_EXTENSIONS
            .iter()
            .any(|ext| candidate.ends_with(ext))
            && candidate.len() > 1
        {
            return Some(candidate.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_patch() -> PatchV1 {
        PatchV1 {
            version: PATCH_CONTRACT_VERSION.to_string(),
            target_file: "src/calc.rs".to_string(),
            context_before: "a + b".to_string(),
            replacement: "a * b".to_string(),
            reason: "multiply, not add".to_string(),
            validation: None,
        }
    }

    #[test]
    fn valid_patch_passes_shape_validation() {
        assert!(validate_patch_shape(&valid_patch()).is_ok());
    }

    #[test]
    fn wrong_version_is_rejected() {
        let mut p = valid_patch();
        p.version = "diff_v9".to_string();
        assert_eq!(
            validate_patch_shape(&p),
            Err(PatchShapeError::WrongVersion("diff_v9".to_string()))
        );
    }

    #[test]
    fn no_op_patch_is_rejected() {
        let mut p = valid_patch();
        p.replacement = p.context_before.clone();
        assert_eq!(validate_patch_shape(&p), Err(PatchShapeError::NoOpPatch));
    }

    #[test]
    fn empty_fields_are_rejected() {
        let mut p = valid_patch();
        p.target_file = "  ".to_string();
        assert_eq!(
            validate_patch_shape(&p),
            Err(PatchShapeError::EmptyField("target_file"))
        );
        let mut p = valid_patch();
        p.context_before = String::new();
        assert_eq!(
            validate_patch_shape(&p),
            Err(PatchShapeError::EmptyField("context_before"))
        );
        let mut p = valid_patch();
        p.reason = String::new();
        assert_eq!(
            validate_patch_shape(&p),
            Err(PatchShapeError::EmptyField("reason"))
        );
    }

    #[test]
    fn path_traversal_is_rejected() {
        let mut p = valid_patch();
        p.target_file = "../../etc/passwd".to_string();
        assert_eq!(
            validate_patch_shape(&p),
            Err(PatchShapeError::PathTraversal(
                "../../etc/passwd".to_string()
            ))
        );
    }

    #[test]
    fn grounding_requires_exactly_one_occurrence() {
        let p = valid_patch();
        assert_eq!(validate_patch_against_content(&p, "x a + b y"), Ok(1));
        assert_eq!(
            validate_patch_against_content(&p, "nothing here"),
            Err(PatchGroundingError::ContextNotFound)
        );
        assert_eq!(
            validate_patch_against_content(&p, "a + b and a + b"),
            Err(PatchGroundingError::ContextAmbiguous(2))
        );
    }

    #[test]
    fn parse_patch_accepts_valid_json() {
        let text = r#"{"version":"patch_v1","target_file":"a.rs","context_before":"x","replacement":"y","reason":"r"}"#;
        let patch = parse_patch(text).expect("valid patch must parse");
        assert_eq!(patch.target_file, "a.rs");
        assert_eq!(patch.reason, "r");
        assert!(patch.validation.is_none());
    }

    #[test]
    fn parse_patch_accepts_json_inside_prose() {
        let text = r#"Here is the patch: {"version":"patch_v1","target_file":"a.rs","context_before":"x","replacement":"y","reason":"r"} hope that helps"#;
        assert!(parse_patch(text).is_ok());
    }

    #[test]
    fn parse_patch_rejects_garbage() {
        assert!(parse_patch("mock-llm-response-to: hello").is_err());
        assert!(parse_patch("```diff\n-old\n+new\n```").is_err());
    }

    #[test]
    fn parse_patch_rejects_schema_violations() {
        // missing required fields
        assert!(parse_patch(r#"{"version":"patch_v1"}"#).is_err());
    }

    #[test]
    fn extract_target_file_prefers_path_tokens() {
        let text = "В файле /tmp/fixture/calc.py функция add ошибается";
        assert_eq!(
            extract_target_file(text),
            Some("/tmp/fixture/calc.py".to_string())
        );
    }

    #[test]
    fn extract_target_file_handles_punctuation_and_bare_names() {
        assert_eq!(
            extract_target_file("fix the bug in calc.rs: it adds"),
            Some("calc.rs".to_string())
        );
        assert_eq!(
            extract_target_file("patch src/main.rs, then rebuild"),
            Some("src/main.rs".to_string())
        );
    }

    #[test]
    fn extract_target_file_ignores_urls_and_missing_paths() {
        assert_eq!(extract_target_file("see https://x.com/a/b"), None);
        assert_eq!(extract_target_file("fix the bug"), None);
    }
}
