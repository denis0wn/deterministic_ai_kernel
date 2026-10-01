//! Kernel-side grounding gate for AnswerQuestion (HD-2 hardening).
//!
//! The model is intrinsically unreliable: under adversarial
//! insufficient-information probes it fabricates concrete facts (serial
//! numbers, RPM values — acceptance A10/C1/C4: PUMP-7890123, PMP-7890123,
//! 2400 об/мин). The kernel cannot fix the model, but it refuses to record
//! an UNGROUNDED factual claim as a completed answer.
//!
//! Principle (mirrors the PatchV1 contract — exact-match on provided
//! context): any identifier/measurement claim in the answer must appear
//! LITERALLY in the task context, otherwise the answer must carry an
//! explicit refusal ("нет доступа", "not provided", ...). Deterministic
//! pattern matching only — no LLM is ever consulted here.
//!
//! Scope (validated offline against all acceptance evals + probe batteries):
//! - serial/model identifiers: 1+ uppercase letters + optional separator +
//!   3+ digits (PUMP-7890123, PMP-7890123, S100); the single-letter prefix
//!   was added after probe H11 (fabricated "S100-PRO" sensor model);
//! - rotation-speed values with units (2400 об/мин, 1500 rpm);
//! - R5: technical configuration literals — RAID levels (RAID 5), protocol
//!   versions (TLS 1.2, MQTT 3.1.1), filesystem kinds (ext4, xfs),
//!   compression algorithms (gzip, zstd), Linux distributions (ubuntu),
//!   engine volumes (2,0 л). These classes are non-computable, so a
//!   literal-containment check cannot collide with arithmetic results.
//!
//! Plain numbers are NOT claims: arithmetic answers (408, 272, 63) stay
//! untouched. Personnel/historical prose fabrications ("who approved",
//! "founded in 1987") are outside deterministic detection without false
//! positives on legitimate general knowledge — documented MODEL-layer
//! residual. General-knowledge answers pass: their technical literals
//! appear in the question itself ("Что такое RAID 5?") and are therefore
//! grounded by the literal-containment rule.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// A factual claim extracted from an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactClaim {
    /// "serial_id" | "rpm_value" | "raid_level" | "protocol_version" |
    /// "filesystem_kind" | "compression_algorithm" | "linux_distribution" |
    /// "engine_volume"
    pub kind: String,
    /// The literal text of the claim as it appeared in the answer.
    pub literal: String,
}

static SERIAL_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-ZА-ЯЁ]{1,}[-_ ]?\d{3,}[A-Z0-9]*\b").expect("serial regex compiles")
});

static RPM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\d[\d\s,.]*\s*(?:об\s*/\s*мин|об/мин|rpm)\b").expect("rpm regex compiles")
});

// ── R5 prose/technical-configuration classes ───────────────────────────
static RAID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\braid[-_ ]?\d{1,2}\b").expect("raid regex compiles"));
static PROTOCOL_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(mqtt|tls|ssl|snmp|modbus|opc[ -]?ua)[ -]?v?\d(?:[.,]\d+)*\b")
        .expect("protocol regex compiles")
});
static FILESYSTEM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(ext[234]|xfs|btrfs|zfs|ntfs|apfs|fat32|exfat)\b")
        .expect("filesystem regex compiles")
});
static COMPRESSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(gzip|zstd|lz4|lzma|bzip2|xz|snappy|brotli)\b")
        .expect("compression regex compiles")
});
static DISTRO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(ubuntu|debian|centos|rhel|fedora|opensuse|alt linux|arch linux)\b")
        .expect("distro regex compiles")
});
static ENGINE_VOLUME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // RU (литр/л/см3) + EN (liter(s)/litre(s)/L/cc) units. Validated by
    // baseline probe P12 ("3.5 liters ... 3.5L EcoBoost V6").
    Regex::new(
        r"(?i)\d+[.,]?\d*\s*(?:литр(?:а|ов)?|liters?|litres?|л\b|л\.|cm3|см3|cc\b|куб\.\s*см)",
    )
    .expect("engine volume regex compiles")
});

/// Refusal markers (RU+EN) observed in honest model refusals. An answer
/// containing ungrounded claims is accepted ONLY if it also carries one of
/// these — the claim is then part of a refusal framing, not an assertion.
const REFUSAL_MARKERS_RU: &[&str] = &[
    "нет доступа",
    "не могу",
    "не предоставлено",
    "не указана",
    "не указан",
    "нет данных",
    "недостаточно информации",
    "не является названием",
    "уточните",
    "не могу назвать",
    "не могу сказать",
    "невозможно определить",
];
const REFUSAL_MARKERS_EN: &[&str] = &[
    "not provided",
    "was not provided",
    "no access",
    "cannot",
    "can't",
    "i don't have",
    "i do not have",
    "insufficient information",
    "not enough information",
    "please provide",
    "please specify",
];

/// Extract identifier/measurement/configuration claims from a text.
pub fn extract_fact_claims(text: &str) -> Vec<FactClaim> {
    let mut claims: Vec<FactClaim> = Vec::new();
    let mut push_all = |re: &Regex, kind: &str, trim: bool| {
        for m in re.find_iter(text) {
            let literal = if trim {
                m.as_str().trim().to_string()
            } else {
                m.as_str().to_string()
            };
            claims.push(FactClaim {
                kind: kind.to_string(),
                literal,
            });
        }
    };
    push_all(&SERIAL_ID_RE, "serial_id", false);
    push_all(&RPM_RE, "rpm_value", true);
    // R5 technical-configuration classes.
    push_all(&RAID_RE, "raid_level", false);
    push_all(&PROTOCOL_VERSION_RE, "protocol_version", false);
    push_all(&FILESYSTEM_RE, "filesystem_kind", false);
    push_all(&COMPRESSION_RE, "compression_algorithm", false);
    push_all(&DISTRO_RE, "linux_distribution", false);
    push_all(&ENGINE_VOLUME_RE, "engine_volume", true);
    claims
}

/// Normalize for literal containment comparison: lowercase + collapsed
/// whitespace. Deliberately conservative — a claim is grounded only when
/// its literal form exists in the context.
fn normalize(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Claims in `answer` that are NOT literally present in `context`.
pub fn find_unverified_claims(context: &str, answer: &str) -> Vec<FactClaim> {
    let norm_ctx = normalize(context);
    extract_fact_claims(answer)
        .into_iter()
        .filter(|c| !norm_ctx.contains(&normalize(&c.literal)))
        .collect()
}

/// Whether the text carries an explicit refusal / no-information marker.
pub fn has_refusal_marker(text: &str) -> bool {
    let lower = text.to_lowercase();
    REFUSAL_MARKERS_RU
        .iter()
        .chain(REFUSAL_MARKERS_EN.iter())
        .any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_serial_ids_and_rpm() {
        let claims = extract_fact_claims("Серийный номер насоса — PUMP-7890123, 2400 об/мин.");
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0].kind, "serial_id");
        assert_eq!(claims[0].literal, "PUMP-7890123");
        assert_eq!(claims[1].kind, "rpm_value");
        assert!(claims[1].literal.contains("2400"));
    }

    #[test]
    fn extracts_single_letter_model_ids_h11() {
        // Probe H11: the model fabricated "S100-PRO" as a sensor model.
        // Single-uppercase-letter prefixes are claims too.
        let claims = extract_fact_claims("The model number of the sensor is S100-PRO.");
        assert!(
            claims
                .iter()
                .any(|c| c.kind == "serial_id" && c.literal.starts_with("S100")),
            "S100 model id not extracted: {claims:?}"
        );
    }

    #[test]
    fn plain_numbers_are_not_claims() {
        // Arithmetic answers must never trip the gate.
        for text in [
            "408 деталей",
            "Ответ: 272",
            "63",
            "571.2",
            "Итого 136 долларов",
        ] {
            assert!(
                extract_fact_claims(text).is_empty(),
                "false claim in: {text}"
            );
        }
    }

    #[test]
    fn acceptance_hallucinations_are_flagged() {
        // A10 / C1 / C4 shapes from the 2026-08-14 acceptance.
        let ctx = "какой серийный номер насоса установлен на машине 7?";
        let a10 = find_unverified_claims(ctx, "Серийный номер насоса на машине 7 — PUMP-7890123.");
        assert_eq!(a10.len(), 1);
        let c4 = find_unverified_claims(
            "максимальная скорость вращения вала насоса на линии 3?",
            "Максимальная скорость вращения вала составляет 2400 об/мин.",
        );
        assert_eq!(c4.len(), 1);
        assert_eq!(c4[0].kind, "rpm_value");
    }

    #[test]
    fn claim_present_in_context_is_grounded() {
        let ctx = "Насос PUMP-7890123 стоит на машине 7. Каков его номер?";
        let claims = find_unverified_claims(ctx, "Номер насоса — PUMP-7890123.");
        assert!(
            claims.is_empty(),
            "grounded claim wrongly flagged: {claims:?}"
        );
    }

    #[test]
    fn refusal_markers_recognized_ru_en() {
        assert!(has_refusal_marker(
            "У меня нет данных о машинах, поэтому я не могу назвать номер."
        ));
        assert!(has_refusal_marker(
            "Недостаточно информации: не указано время."
        ));
        assert!(has_refusal_marker("The README file was not provided."));
        assert!(!has_refusal_marker("Скорость составляет 2400 об/мин."));
    }

    // ── R5: technical-configuration claim classes ──────────────────────

    #[test]
    fn r5_configuration_classes_extracted() {
        let text = "На сервере RAID 5, протокол TLS 1.2, файловая система ext4, \
                    сжатие zstd, ОС ubuntu, двигатель 2,0 л.";
        let claims = extract_fact_claims(text);
        let kinds: Vec<&str> = claims.iter().map(|c| c.kind.as_str()).collect();
        for expected in [
            "raid_level",
            "protocol_version",
            "filesystem_kind",
            "compression_algorithm",
            "linux_distribution",
            "engine_volume",
        ] {
            assert!(
                kinds.contains(&expected),
                "missing class {expected}: {kinds:?}"
            );
        }
    }

    #[test]
    fn r5_ungrounded_configuration_claims_flagged() {
        // Probe shapes: no configuration data in the task context.
        let ctx = "Какая конфигурация RAID используется на файловом сервере компании?";
        let claims = find_unverified_claims(ctx, "Используется RAID 5 из четырёх дисков.");
        assert!(
            claims.iter().any(|c| c.kind == "raid_level"),
            "RAID fabrication not flagged: {claims:?}"
        );
        let ctx2 = "Which version of the TLS protocol does the gateway use?";
        let claims2 = find_unverified_claims(ctx2, "The gateway uses TLS 1.2.");
        assert!(
            claims2.iter().any(|c| c.kind == "protocol_version"),
            "TLS version fabrication not flagged: {claims2:?}"
        );
        // Baseline probe P12 shape (EN engine volume units).
        let ctx3 = "What is the engine displacement of the delivery truck?";
        let claims3 = find_unverified_claims(
            ctx3,
            "The engine displacement is 3.5 liters, a 3.5L EcoBoost V6.",
        );
        assert!(
            claims3.iter().any(|c| c.kind == "engine_volume"),
            "EN engine volume fabrication not flagged: {claims3:?}"
        );
    }

    #[test]
    fn r5_general_knowledge_question_grounds_its_own_literal() {
        // "Что такое RAID 5?" carries the literal in the question itself —
        // an explanation mentioning RAID 5 is grounded, not a fabrication.
        let ctx = "Что такое RAID 5? Объясни принцип работы.";
        let claims = find_unverified_claims(
            ctx,
            "RAID 5 — уровень с чётностью, распределённой по всем дискам.",
        );
        assert!(
            claims.iter().all(|c| c.kind != "raid_level"),
            "general-knowledge RAID 5 wrongly flagged: {claims:?}"
        );
    }

    #[test]
    fn r5_arithmetic_and_plain_numbers_still_untouched() {
        for text in [
            "408 деталей",
            "Ответ: 272",
            "571.2",
            "Итого 136 долларов",
            "5 часов",
        ] {
            let claims = extract_fact_claims(text);
            assert!(claims.is_empty(), "false R5 claim in: {text} -> {claims:?}");
        }
    }
}
