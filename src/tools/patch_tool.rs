//! `apply_patch_v1` — the authorized patch-application tool (P2).
//!
//! Registered in `tools::registry` and reachable ONLY through
//! `execute_tool`, the single enforcement boundary: unknown names fail
//! closed and this mutating tool requires explicit confirmation. The kernel
//! effect path confirms with its own policy authorization (the patch must
//! already have passed kernel-side validation); no LLM or UI input can
//! trigger application directly.
//!
//! No shell, no `sh -c`, no external patch utilities — application is pure
//! Rust via `execution::patch_apply`.

use crate::execution::patch_apply;
use crate::execution::patch_contract::PatchV1;

pub async fn apply_patch(
    args: &serde_json::Value,
    workspace: &str,
) -> Result<serde_json::Value, String> {
    let patch_value = args
        .get("patch")
        .ok_or("missing 'patch' (PatchV1 object)")?;
    let patch: PatchV1 = serde_json::from_value(patch_value.clone())
        .map_err(|e| format!("patch_v1 schema violation: {e}"))?;

    let evidence = patch_apply::apply_patch_v1(&patch, workspace)?;

    Ok(serde_json::json!({
        "tool": "apply_patch_v1",
        "status": "applied",
        "evidence": evidence,
    }))
}
