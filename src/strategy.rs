//! Strategy taxonomy skeleton (PROGRESS UNTIL VERIFIED stage 3).
//!
//! Strategies are KERNEL-ENUMERATED constants. The mapping from a
//! failure signature to admissible strategies is kernel configuration
//! that grows ONLY with demonstrated evidence (work-selection
//! principle): a new mapping requires a reproducible failure the
//! strategy can actually fix. The LLM never selects strategies; it
//! only fills the chosen strategy with candidate content (stage 3 is
//! advisory — selection/enforcement is a later stage, and no model
//! feedback of any kind exists here).

/// Kernel-owned strategy space (research §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Strategy {
    /// Re-read / re-snapshot the target — eliminates stale grounding.
    ReGround,
    /// Kernel-owned payload reformulation from evidence-gated templates.
    RepresentationChange,
    /// S3 deterministic encoding repair (escape-corruption class).
    EncodingRepair,
    /// Kernel-approved decomposition with a composition contract.
    Decomposition,
    /// Additional deterministic checks (probes, oracle templates).
    EvidenceAcquisition,
    /// Verifier-gap proof + human grant of a new verifier.
    VerifierGapEscalation,
    /// Human decision gate (existing review_gate).
    HumanEscalation,
}

/// Deterministic mapping failure signature → admissible strategies.
///
/// v1 mappings (each backed by demonstrated evidence):
/// - "patch_v1 schema violation: invalid escape" → EncodingRepair
///   (NorthPay C4 corpus: 3/3 reproducible; repair proven by design
///   review CONDITIONAL GO and stage-3 acceptance), with
///   HumanEscalation as the universal fallback.
/// - anything else → HumanEscalation only (no demonstrated strategy).
pub fn admissible_strategies(failure_signature: &str) -> Vec<Strategy> {
    if failure_signature.contains("patch_v1 schema violation: invalid escape") {
        return vec![Strategy::EncodingRepair, Strategy::HumanEscalation];
    }
    vec![Strategy::HumanEscalation]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_corruption_maps_to_encoding_repair() {
        let sig = "fatal: malformed patch: patch_v1 schema violation: invalid escape at line 1 column 290";
        let s = admissible_strategies(sig);
        assert_eq!(s[0], Strategy::EncodingRepair);
        assert!(s.contains(&Strategy::HumanEscalation));
    }

    #[test]
    fn unknown_signatures_escalate() {
        let s = admissible_strategies("fatal: real tests failed with exit code 1");
        assert_eq!(s, vec![Strategy::HumanEscalation]);
    }

    #[test]
    fn mapping_is_deterministic() {
        let sig = "patch_v1 schema violation: invalid escape";
        assert_eq!(admissible_strategies(sig), admissible_strategies(sig));
    }
}
