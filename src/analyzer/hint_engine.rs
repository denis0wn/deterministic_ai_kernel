//! Hint engine — Layer 1 of self-guiding task synthesis.
//!
//! The analyzer, based on the RULE that produced a finding, generates
//! deterministic guidance hints describing what the fix will likely need.
//! This removes the need for a human to hand-craft hints per task: the
//! system derives the guidance from the rule itself (no LLM involved).
//!
//! These hints are advisory context for the model. They do NOT weaken any
//! gate: the model's patch still passes through context-verified apply +
//! real tests + fail-closed validation. A hint can help the model produce
//! a correct patch, but it can never make a wrong patch pass.

/// Generate fix hints for a finding based on its rule id.
///
/// Deterministic: same rule id ⇒ same hints. Rules without a hint profile
/// return an empty list (the task still proceeds, just without hints).
pub fn generate_hints(rule_id: &str) -> Vec<String> {
    let hints: &[&str] = match rule_id {
        "money-truncation" => &[
            "The defect truncates money (e.g. int() drops sub-cent fractions). The fix needs exact decimal arithmetic: use the `decimal` module (`Decimal`, `ROUND_HALF_UP`).",
            "Preserve the function's existing return type: if it returned `float`, convert the decimal result back with `float(...)` before returning — a `Decimal` return breaks equality checks and callers.",
            "Round HALF-UP to the cent via `Decimal.quantize(..., rounding=ROUND_HALF_UP)` — builtin `round()` has no `rounding` keyword (`round(x, 2, rounding=...)` raises TypeError). You may add `from decimal import Decimal, ROUND_HALF_UP` INSIDE the function you are fixing.",
        ],
        "money-round-bare" => &[
            "The defect is a bare round() with no rounding policy. Use an explicit policy, e.g. `decimal` with `ROUND_HALF_UP`.",
            "Preserve the function's existing return type: if it returned `float`, convert the decimal result back with `float(...)` before returning — a `Decimal` return breaks equality checks and callers.",
            "Round HALF-UP to the cent via `Decimal.quantize(..., rounding=ROUND_HALF_UP)`; builtin `round()` has no `rounding` keyword. You may add the import INSIDE the function you are fixing.",
        ],
        // Validated by measurement (mq_dateflow_* 2026-09-26): v1 of this
        // recipe described the iteration and produced count-and-skip loops
        // landing on weekends (3/8). The invariant form below converts 8/8.
        "business-days" => &[
            "The defect counts calendar days. Correct algorithm: start from the date; while added < days, advance one calendar day, and only when it lands on a business day (weekday() < 5) increment added. Only business-day landings count toward the requested number of days. Never count calendar days traversed.",
            "Preserve the function's existing return type: a date in, a date out.",
            "Stdlib only (datetime). Do not import holiday calendars or numpy.",
        ],
        "floor-div-money" => &[
            "The defect is floor division (`//`) truncating a monetary amount. Decide the correct policy: exact division or explicit HALF-UP rounding.",
            "If you need `decimal`, you may import it INSIDE the function to keep the patch in one contiguous region.",
        ],
        "none-arith" => &[
            "The defect is arithmetic on a value that may be None (dict.get without a default). Provide a default (e.g. `.get(key, 0)`) or handle None before the arithmetic.",
        ],
        "offbyone-range" => &[
            "The defect is an off-by-one in a range (the loop may skip the first element). Check whether the range should start at 0 instead of 1.",
        ],
        // No hint profile for other rules (e.g. todo-marker, dangerous-eval):
        // the task proceeds without hints.
        _ => &[],
    };
    hints.iter().map(|s| s.to_string()).collect()
}

/// True when a rule has a hint profile (for tests / reporting).
pub fn has_hints(rule_id: &str) -> bool {
    !generate_hints(rule_id).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_rules_have_hints() {
        for rule in [
            "money-truncation",
            "money-round-bare",
            "floor-div-money",
            "none-arith",
            "offbyone-range",
            "business-days",
        ] {
            assert!(has_hints(rule), "{rule} should have hints");
            assert!(!generate_hints(rule).is_empty());
        }
    }

    #[test]
    fn business_days_hints_state_the_invariant() {
        // Validated 2026-09-26 (mq_dateflow_hints2): recipes must state the
        // invariant (only business-day landings count), not the iteration.
        let hints = generate_hints("business-days").join(" ");
        assert!(hints.contains("Only business-day landings count"));
        assert!(hints.contains("weekday() < 5"));
    }

    #[test]
    fn non_hint_rules_return_empty() {
        for rule in [
            "todo-marker",
            "dangerous-eval",
            "float-equality",
            "unknown-rule",
        ] {
            assert!(!has_hints(rule), "{rule} should have no hints");
        }
    }

    #[test]
    fn hints_are_deterministic() {
        assert_eq!(
            generate_hints("money-truncation"),
            generate_hints("money-truncation")
        );
    }

    #[test]
    fn truncation_hints_mention_decimal_and_inline_import() {
        let hints = generate_hints("money-truncation").join(" ");
        assert!(hints.contains("decimal") || hints.contains("Decimal"));
        assert!(hints.contains("INSIDE the function"));
        assert!(hints.contains("HALF-UP"));
        // E0 2026-09-25: hints without these two guards produced 14/14
        // failures (Decimal return broke float-equality; builtin
        // round(..., rounding=) raised TypeError).
        assert!(hints.contains("return type"));
        assert!(hints.contains("quantize"));
    }
}
