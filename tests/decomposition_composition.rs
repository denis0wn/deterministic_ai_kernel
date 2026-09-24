//! PROGRESS UNTIL VERIFIED — stage 4.
//!
//! Composition property tests for decomposition: two DISJOINT patch_v1
//! regions of the same file compose through sequential kernel apply
//! (each grounded exactly once at its apply moment), and a patch that
//! spans both regions can no longer be applied after composition —
//! i.e. the composition is a real state change, not a reorderable
//! no-op. Pure deterministic tests; no model, no DB.

use deterministic_ai_kernel::execution::patch_apply::apply_patch_v1;
use deterministic_ai_kernel::execution::patch_contract::PatchV1;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn fresh_workspace(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("dak_decomp_{name}_{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn patch(target: &str, context: &str, replacement: &str) -> PatchV1 {
    PatchV1 {
        version: "patch_v1".to_string(),
        target_file: target.to_string(),
        context_before: context.to_string(),
        replacement: replacement.to_string(),
        reason: "composition test".to_string(),
        validation: None,
    }
}

const ORIGINAL: &str = "# MARKER_A\ndef refund(x):\n    return int(x * 100) / 100\n";

#[test]
fn disjoint_regions_compose_sequentially() {
    let ws = fresh_workspace("compose_ok");
    let file = ws.join("dual.py");
    fs::write(&file, ORIGINAL).unwrap();
    let target = file.to_str().unwrap().to_string();

    // Lemma patch: top region only (consumes its marker).
    let a = patch(
        &target,
        "# MARKER_A\n",
        "from decimal import Decimal, ROUND_HALF_UP\n",
    );
    // Carrier patch: function region only.
    let b = patch(
        &target,
        "    return int(x * 100) / 100\n",
        "    return float(Decimal(str(x)).quantize(Decimal('0.01'), rounding=ROUND_HALF_UP))\n",
    );

    apply_patch_v1(&a, ws.to_str().unwrap()).expect("lemma patch must apply");
    let mid = fs::read_to_string(&file).unwrap();
    assert!(mid.contains("from decimal import"));
    assert!(
        mid.contains("int(x * 100)"),
        "function region untouched yet"
    );

    apply_patch_v1(&b, ws.to_str().unwrap()).expect("carrier patch must apply after lemma");
    let final_content = fs::read_to_string(&file).unwrap();
    assert!(final_content.contains("from decimal import"));
    assert!(final_content.contains("quantize"));
    assert!(!final_content.contains("int(x * 100)"), "defect replaced");

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn lemma_region_cannot_be_reapplied_after_composition() {
    let ws = fresh_workspace("no_reapply");
    let file = ws.join("dual.py");
    fs::write(&file, ORIGINAL).unwrap();
    let target = file.to_str().unwrap().to_string();

    let a = patch(
        &target,
        "# MARKER_A\n",
        "from decimal import Decimal, ROUND_HALF_UP\n",
    );
    apply_patch_v1(&a, ws.to_str().unwrap()).expect("first apply must succeed");
    let again = apply_patch_v1(&a, ws.to_str().unwrap());
    assert!(
        again.is_err(),
        "re-applying the consumed lemma region must fail grounding"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn overlapping_patch_is_invalidated_by_composition() {
    let ws = fresh_workspace("overlap_invalidated");
    let file = ws.join("dual.py");
    fs::write(&file, ORIGINAL).unwrap();
    let target = file.to_str().unwrap().to_string();

    // A patch spanning BOTH regions (the single-attempt expression of
    // the whole fix).
    let spanning = patch(
        &target,
        ORIGINAL,
        "from decimal import Decimal, ROUND_HALF_UP\ndef refund(x):\n    return float(Decimal(str(x)).quantize(Decimal('0.01'), rounding=ROUND_HALF_UP))\n",
    );

    // Before decomposition it applies cleanly...
    let pre = apply_patch_v1(&spanning, ws.to_str().unwrap());
    assert!(pre.is_ok(), "spanning patch valid before decomposition");
    // ...restore, then compose the disjoint regions...
    fs::write(&file, ORIGINAL).unwrap();
    let a = patch(
        &target,
        "# MARKER_A\n",
        "from decimal import Decimal, ROUND_HALF_UP\n",
    );
    let b = patch(
        &target,
        "    return int(x * 100) / 100\n",
        "    return float(Decimal(str(x)).quantize(Decimal('0.01'), rounding=ROUND_HALF_UP))\n",
    );
    apply_patch_v1(&a, ws.to_str().unwrap()).unwrap();
    apply_patch_v1(&b, ws.to_str().unwrap()).unwrap();

    // ...and now the spanning patch no longer grounds: the composition
    // is a genuine state change, proving regions were actually edited.
    let post = apply_patch_v1(&spanning, ws.to_str().unwrap());
    assert!(
        post.is_err(),
        "spanning patch must not ground after disjoint composition"
    );

    let _ = fs::remove_dir_all(&ws);
}
