//! Phase 4 §3 — Artifact Immutability
//!
//! Verifies that the ArtifactRegistry is append-only: records are never
//! removed or mutated after registration, and hashes remain stable.

use deterministic_ai_kernel::registry::{ArtifactRecord, ArtifactRegistry, ArtifactType};
use deterministic_ai_kernel::workflow::semantic::bias::BiasVersion;

fn make_record(artifact_type: ArtifactType, payload: &[u8]) -> ArtifactRecord {
    ArtifactRecord::new(artifact_type, 42, BiasVersion::V1, None, payload)
}

#[test]
fn registered_artifact_hash_is_stable() {
    let mut reg = ArtifactRegistry::new();
    let record = make_record(ArtifactType::VerificationVerdict, b"payload_a");
    let expected_hash = record.hash.clone();
    let id = reg.register(record);

    let fetched = reg.get(&id).expect("record must exist after registration");
    assert_eq!(fetched.hash, expected_hash, "hash must not change after registration");
}

#[test]
fn two_records_with_same_payload_have_same_hash() {
    let r1 = make_record(ArtifactType::Snapshot, b"same_payload");
    let r2 = make_record(ArtifactType::Snapshot, b"same_payload");
    assert_eq!(r1.hash, r2.hash, "BLAKE3 of identical payloads must match");
}

#[test]
fn two_records_with_different_payloads_have_different_hashes() {
    let r1 = make_record(ArtifactType::Snapshot, b"payload_a");
    let r2 = make_record(ArtifactType::Snapshot, b"payload_b");
    assert_ne!(r1.hash, r2.hash, "distinct payloads must produce distinct hashes");
}

#[test]
fn unregistered_id_returns_none() {
    let reg = ArtifactRegistry::new();
    let phantom_id = uuid::Uuid::new_v4();
    assert_eq!(reg.get(&phantom_id), None);
}

#[test]
fn multiple_distinct_artifacts_coexist() {
    let mut reg = ArtifactRegistry::new();
    let id1 = reg.register(make_record(ArtifactType::VerificationPlan, b"plan"));
    let id2 = reg.register(make_record(ArtifactType::ReplayCapsule, b"capsule"));
    let id3 = reg.register(make_record(ArtifactType::SemanticBias, b"bias"));

    assert!(reg.get(&id1).is_some());
    assert!(reg.get(&id2).is_some());
    assert!(reg.get(&id3).is_some());
    assert_eq!(reg.len(), 3);
}

#[test]
fn registry_is_append_only_len_never_shrinks() {
    let mut reg = ArtifactRegistry::new();
    assert_eq!(reg.len(), 0);
    reg.register(make_record(ArtifactType::Snapshot, b"a"));
    assert_eq!(reg.len(), 1);
    reg.register(make_record(ArtifactType::Snapshot, b"b"));
    assert_eq!(reg.len(), 2);
}
