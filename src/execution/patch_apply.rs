//! Canonical authorized patch application (P2, H-3 apply link).
//!
//! The kernel — never the LLM — applies a validated `PatchV1` inside an
//! operator-declared workspace. No shell, no external patch utilities, no
//! `git apply`: pure Rust, atomic temp-file + rename replacement, blake3
//! evidence computed kernel-side, and fail-closed behavior at every stage.
//!
//! Five distinct facts are deliberately separated (mission §16):
//! PATCH GENERATED (PatchCode artifact) → PATCH VALIDATED (P1 shape +
//! grounding) → PATCH APPLIED (this module's evidence) → PATCH VERIFIED
//! (P3) → TASK COMPLETED (P4 gate). This module only proves APPLIED.

use crate::execution::patch_contract::PatchV1;
use serde::{Deserialize, Serialize};

/// Kernel-owned proof that a patch was applied. Every field is computed by
/// the kernel from actual filesystem state; nothing here is accepted from
/// the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchApplyEvidence {
    pub target_file: String,
    pub workspace: String,
    pub pre_image_blake3: String,
    pub pre_image_bytes: usize,
    pub post_image_blake3: String,
    pub post_image_bytes: usize,
    pub patch_version: String,
    pub context_blake3: String,
    pub replacement_blake3: String,
}

/// Test-only fault injection point used to prove the rollback path.
/// Not reachable from any production call site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ApplyFault {
    /// Corrupt the expected post-image hash after the rename, forcing the
    /// post-verification mismatch that triggers rollback.
    CorruptExpectedPostHash,
}

fn blake3_hex(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

/// Apply a validated patch atomically inside `workspace`.
///
/// Sequence (mission §4): read current file → re-verify context occurs
/// exactly once (precondition at APPLY time, not just at parse time) →
/// construct new content deterministically → write temp file in the same
/// directory → sync → atomic rename → re-read target → verify post-image
/// hash → return evidence. Any failure before the rename leaves the file
/// untouched; a post-verification failure triggers rollback.
pub fn apply_patch_v1(patch: &PatchV1, workspace: &str) -> Result<PatchApplyEvidence, String> {
    apply_patch_v1_with_fault(patch, workspace, None)
}

/// Fault-injectable core. Production callers use `apply_patch_v1` (no fault).
pub fn apply_patch_v1_with_fault(
    patch: &PatchV1,
    workspace: &str,
    fault: Option<ApplyFault>,
) -> Result<PatchApplyEvidence, String> {
    use crate::execution::patch_contract::{validate_patch_against_content, validate_patch_shape};

    // 0. Re-validate shape at APPLY time. The stored PatchV1 is still
    //    model-originated data; the kernel trusts nothing by age.
    validate_patch_shape(patch).map_err(|e| format!("patch shape invalid at apply: {e}"))?;

    // 1. Confinement: canonical resolution inside the workspace. This
    //    rejects '..', symlink escape and out-of-workspace absolute paths.
    //    resolve_safe canonicalizes symlinks and checks the workspace prefix.
    let canonical = crate::tools::file_tools::resolve_safe(&patch.target_file, workspace)?;
    let canonical_str = canonical.to_string_lossy().into_owned();

    // 2. Read real current content + pre-image evidence (kernel-side hash).
    let content = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("cannot read target '{}': {e}", canonical_str))?;
    let pre_hash = blake3_hex(&content);
    let pre_bytes = content.len();

    // 3. Precondition at apply time: context must occur exactly once in the
    //    CURRENT file. If the file changed between generation and apply
    //    (stale patch / concurrent modification), application is refused.
    validate_patch_against_content(patch, &content)
        .map_err(|e| format!("apply precondition failed: {e}"))?;

    // 4. Deterministic new content: replace exactly one occurrence.
    let new_content = content.replacen(&patch.context_before, &patch.replacement, 1);
    let mut expected_post_hash = blake3_hex(&new_content);
    if expected_post_hash == pre_hash {
        return Err("apply refused: resulting content identical to pre-image (no-op)".into());
    }

    // 5. Atomic replacement: temp file in the SAME directory (same
    //    filesystem), full write + sync, then rename over the target.
    let dir = canonical
        .parent()
        .ok_or_else(|| "target has no parent directory".to_string())?;
    let file_name = canonical
        .file_name()
        .ok_or_else(|| "target has no file name".to_string())?
        .to_string_lossy();
    let unique = format!(
        "{}.dak_apply_tmp_{}_{}",
        file_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let temp_path = dir.join(unique);

    let write_temp = |bytes: &str| -> Result<(), String> {
        use std::io::Write;
        let mut f =
            std::fs::File::create(&temp_path).map_err(|e| format!("temp create failed: {e}"))?;
        f.write_all(bytes.as_bytes())
            .map_err(|e| format!("temp write failed: {e}"))?;
        f.sync_all().map_err(|e| format!("temp sync failed: {e}"))?;
        Ok(())
    };

    if let Err(e) = write_temp(&new_content) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&temp_path, &canonical) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("atomic rename failed (file untouched): {e}"));
    }

    // 6. Post-verification: re-read the target and compare against the
    //    expected post-image hash. Success is never declared from the rename
    //    alone (mission §6).
    if fault == Some(ApplyFault::CorruptExpectedPostHash) {
        expected_post_hash = format!("corrupted-{expected_post_hash}");
    }
    let reread = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("post-verification read failed: {e}"))?;
    let actual_post_hash = blake3_hex(&reread);
    if actual_post_hash != expected_post_hash {
        // 7. Rollback: restore the pre-image via the same atomic path.
        //    Risk window (documented, mission §9): between the successful
        //    rename above and a completed rollback rename, concurrent
        //    readers may observe the post-image. If the rollback itself
        //    fails, the error reports the exact inconsistent state with
        //    both hashes — never a fake success.
        if let Err(re) = write_temp(&content) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "post-verification FAILED (expected {expected_post_hash}, got {actual_post_hash}) \
                 and rollback write failed ({re}); target '{}' may hold post-image",
                canonical_str
            ));
        }
        if let Err(re) = std::fs::rename(&temp_path, &canonical) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "post-verification FAILED and rollback rename failed ({re}); \
                 target '{}' may hold post-image (pre-hash {pre_hash})",
                canonical_str
            ));
        }
        return Err(format!(
            "post-verification FAILED (expected {expected_post_hash}, got {actual_post_hash}); \
             rolled back to pre-image {pre_hash}"
        ));
    }

    Ok(PatchApplyEvidence {
        target_file: canonical_str,
        workspace: std::path::Path::new(workspace)
            .canonicalize()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| workspace.to_string()),
        pre_image_blake3: pre_hash,
        pre_image_bytes: pre_bytes,
        post_image_blake3: actual_post_hash,
        post_image_bytes: reread.len(),
        patch_version: patch.version.clone(),
        context_blake3: blake3_hex(&patch.context_before),
        replacement_blake3: blake3_hex(&patch.replacement),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::patch_contract::{PatchV1, PATCH_CONTRACT_VERSION};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Unique disposable workspace per test — never the user's repository.
    fn fresh_workspace() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("dak_p2_ws_{n}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(ws: &std::path::Path, name: &str, content: &str) -> String {
        let path = ws.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn patch_for(target: &str, context: &str, replacement: &str) -> PatchV1 {
        PatchV1 {
            version: PATCH_CONTRACT_VERSION.to_string(),
            target_file: target.to_string(),
            context_before: context.to_string(),
            replacement: replacement.to_string(),
            reason: "test patch".to_string(),
            validation: None,
        }
    }

    const ORIGINAL: &str = "def multiply(a, b):\n    return a + b\n";
    const CONTEXT: &str = "    return a + b";
    const REPLACEMENT: &str = "    return a * b";

    #[test]
    fn a_valid_patch_actually_changes_the_file() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for(&target, CONTEXT, REPLACEMENT);

        let evidence = apply_patch_v1(&patch, ws.to_str().unwrap()).expect("apply succeeds");

        let after = std::fs::read_to_string(&target).unwrap();
        assert_eq!(after, "def multiply(a, b):\n    return a * b\n");
        // Evidence reports the CANONICAL path (on macOS /var -> /private/var).
        assert_eq!(
            evidence.target_file,
            std::fs::canonicalize(&target)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn b_evidence_hashes_are_correct_and_differ() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for(&target, CONTEXT, REPLACEMENT);

        let evidence = apply_patch_v1(&patch, ws.to_str().unwrap()).expect("apply succeeds");

        assert_eq!(evidence.pre_image_blake3, blake3_hex(ORIGINAL));
        assert_eq!(
            evidence.post_image_blake3,
            blake3_hex("def multiply(a, b):\n    return a * b\n")
        );
        assert_ne!(evidence.pre_image_blake3, evidence.post_image_blake3);
        assert_eq!(evidence.pre_image_bytes, ORIGINAL.len());
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn c_missing_context_is_rejected_and_file_untouched() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for(&target, "    return a / b", "    return a * b");

        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("apply precondition failed"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), ORIGINAL);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn d_duplicated_context_is_rejected() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", &format!("{ORIGINAL}{ORIGINAL}"));
        let patch = patch_for(&target, CONTEXT, REPLACEMENT);

        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("apply precondition failed"), "got: {err}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn e_target_outside_workspace_is_rejected() {
        let ws = fresh_workspace();
        let outside_dir = fresh_workspace(); // a different workspace
        let outside_target = fixture(&outside_dir, "outside.py", ORIGINAL);
        let patch = patch_for(&outside_target, CONTEXT, REPLACEMENT);

        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("escapes workspace"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&outside_target).unwrap(), ORIGINAL);
        let _ = std::fs::remove_dir_all(&ws);
        let _ = std::fs::remove_dir_all(&outside_dir);
    }

    #[test]
    fn f_nonexistent_target_is_rejected() {
        let ws = fresh_workspace();
        let missing = ws.join("missing.py");
        let patch = patch_for(missing.to_str().unwrap(), CONTEXT, REPLACEMENT);

        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(
            err.contains("escapes workspace") || err.contains("cannot read target"),
            "got: {err}"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn g_path_traversal_is_rejected() {
        let ws = fresh_workspace();
        // Shape validation catches '..' components before fs access.
        let patch = patch_for("../escape.py", CONTEXT, REPLACEMENT);
        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("path traversal"), "got: {err}");

        // A relative path that walks out via a real subdirectory too.
        let sub = ws.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let patch2 = patch_for("sub/../../escape.py", CONTEXT, REPLACEMENT);
        let err2 = apply_patch_v1(&patch2, ws.to_str().unwrap()).unwrap_err();
        assert!(
            err2.contains("path traversal") || err2.contains("escapes workspace"),
            "got: {err2}"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn h_absolute_path_outside_workspace_is_rejected() {
        let ws = fresh_workspace();
        // Non-noop content so shape validation passes and the confinement
        // check is what rejects the absolute out-of-workspace path.
        let patch = patch_for("/etc/hosts", "localhost", "localhost-pwned");
        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("escapes workspace"), "got: {err}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn i_symlink_escape_is_rejected() {
        let ws = fresh_workspace();
        let outside_dir = fresh_workspace();
        let outside_target = fixture(&outside_dir, "secret.py", ORIGINAL);
        let link = ws.join("innocent.py");
        std::os::unix::fs::symlink(&outside_target, &link).expect("symlink fixture");

        let patch = patch_for(link.to_str().unwrap(), CONTEXT, REPLACEMENT);
        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("escapes workspace"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&outside_target).unwrap(), ORIGINAL);
        let _ = std::fs::remove_dir_all(&ws);
        let _ = std::fs::remove_dir_all(&outside_dir);
    }

    #[test]
    fn j_stale_content_changed_after_generation_is_rejected() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for(&target, CONTEXT, REPLACEMENT);

        // Another process modifies the file between generation and apply.
        std::fs::write(&target, "def multiply(a, b):\n    return a ** b\n").unwrap();

        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("apply precondition failed"), "got: {err}");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "def multiply(a, b):\n    return a ** b\n"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn k_no_op_is_rejected_at_shape_level() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for(&target, CONTEXT, CONTEXT);
        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("no-op"), "got: {err}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn l_malformed_patch_is_revalidated_at_apply_time() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let mut patch = patch_for(&target, CONTEXT, REPLACEMENT);
        patch.version = "not-patch-v1".to_string();
        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(err.contains("patch shape invalid at apply"), "got: {err}");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn m_pre_rename_failure_leaves_file_untouched() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);

        // The valid patch path is the ONLY writer: it succeeds and leaves
        // no temp litter behind.
        let good = patch_for(&target, CONTEXT, REPLACEMENT);
        assert!(apply_patch_v1(&good, ws.to_str().unwrap()).is_ok());

        // Restore the pre-image, then a rejected apply must not mutate the
        // target and must not leave temp files behind.
        std::fs::write(&target, ORIGINAL).unwrap();
        let bad = patch_for(&target, "missing context", REPLACEMENT);
        assert!(apply_patch_v1(&bad, ws.to_str().unwrap()).is_err());
        let litter: Vec<_> = std::fs::read_dir(&ws)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("dak_apply_tmp"))
            .collect();
        assert!(litter.is_empty(), "no temp files may survive a rejection");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), ORIGINAL);
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn n_post_verification_failure_rolls_back_to_pre_image() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for(&target, CONTEXT, REPLACEMENT);

        let err = apply_patch_v1_with_fault(
            &patch,
            ws.to_str().unwrap(),
            Some(ApplyFault::CorruptExpectedPostHash),
        )
        .unwrap_err();
        assert!(err.contains("post-verification FAILED"), "got: {err}");
        assert!(err.contains("rolled back to pre-image"), "got: {err}");
        // The file must be byte-identical to the pre-image.
        assert_eq!(std::fs::read_to_string(&target).unwrap(), ORIGINAL);
        let litter: Vec<_> = std::fs::read_dir(&ws)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("dak_apply_tmp"))
            .collect();
        assert!(litter.is_empty(), "rollback must remove temp files");
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn q_repeat_apply_of_same_patch_is_rejected_safely() {
        let ws = fresh_workspace();
        let target = fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for(&target, CONTEXT, REPLACEMENT);

        apply_patch_v1(&patch, ws.to_str().unwrap()).expect("first apply succeeds");
        let err = apply_patch_v1(&patch, ws.to_str().unwrap()).unwrap_err();
        assert!(
            err.contains("apply precondition failed"),
            "second apply must be rejected: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "def multiply(a, b):\n    return a * b\n"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn r_relative_target_resolves_inside_workspace() {
        let ws = fresh_workspace();
        fixture(&ws, "calc.py", ORIGINAL);
        let patch = patch_for("calc.py", CONTEXT, REPLACEMENT);

        let evidence = apply_patch_v1(&patch, ws.to_str().unwrap()).expect("apply succeeds");
        assert!(evidence.target_file.ends_with("calc.py"));
        assert_eq!(
            std::fs::read_to_string(ws.join("calc.py")).unwrap(),
            "def multiply(a, b):\n    return a * b\n"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }
}
