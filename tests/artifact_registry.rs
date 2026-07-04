use deterministic_ai_kernel::{
    registry::{ArtifactRecord, ArtifactRegistry, ArtifactType},
    workflow::semantic::bias::BiasVersion,
};

fn make_record(seed: u64, parent: Option<uuid::Uuid>) -> ArtifactRecord {
    ArtifactRecord::new(
        ArtifactType::SemanticBias,
        seed,
        BiasVersion::V1,
        parent,
        b"{}",
    )
}

#[test]
fn register_and_retrieve_by_id() {
    let mut reg = ArtifactRegistry::new();
    let rec = make_record(42, None);
    let id = rec.id;
    reg.register(rec);
    assert_eq!(reg.get(&id).unwrap().seed, 42);
}

#[test]
fn find_by_seed_returns_all_matches() {
    let mut reg = ArtifactRegistry::new();
    reg.register(make_record(1, None));
    reg.register(make_record(1, None));
    reg.register(make_record(2, None));
    assert_eq!(reg.find_by_seed(1).len(), 2);
    assert_eq!(reg.find_by_seed(2).len(), 1);
    assert_eq!(reg.find_by_seed(99).len(), 0);
}

#[test]
fn find_by_type_filters_correctly() {
    let mut reg = ArtifactRegistry::new();
    reg.register(make_record(1, None));
    let snap = ArtifactRecord::new(
        ArtifactType::Snapshot,
        0,
        BiasVersion::V1,
        None,
        b"snap",
    );
    reg.register(snap);
    assert_eq!(reg.find_by_type(&ArtifactType::SemanticBias).len(), 1);
    assert_eq!(reg.find_by_type(&ArtifactType::Snapshot).len(), 1);
    assert_eq!(reg.find_by_type(&ArtifactType::ReplayCapsule).len(), 0);
}

#[test]
fn hash_is_deterministic_for_same_payload() {
    let a = ArtifactRecord::new(ArtifactType::SemanticBias, 7, BiasVersion::V1, None, b"payload");
    let b = ArtifactRecord::new(ArtifactType::SemanticBias, 7, BiasVersion::V1, None, b"payload");
    assert_eq!(a.hash, b.hash);
}

#[test]
fn hash_differs_for_different_payloads() {
    let a = ArtifactRecord::new(ArtifactType::SemanticBias, 7, BiasVersion::V1, None, b"aaa");
    let b = ArtifactRecord::new(ArtifactType::SemanticBias, 7, BiasVersion::V1, None, b"bbb");
    assert_ne!(a.hash, b.hash);
}

#[test]
fn parent_chain_is_preserved() {
    let mut reg = ArtifactRegistry::new();
    let root = make_record(1, None);
    let root_id = root.id;
    reg.register(root);
    let child = make_record(2, Some(root_id));
    let child_id = child.id;
    reg.register(child);
    assert_eq!(reg.get(&child_id).unwrap().parent, Some(root_id));
}

#[test]
fn registry_is_append_only() {
    let mut reg = ArtifactRegistry::new();
    let id = reg.register(make_record(1, None));
    let before = reg.get(&id).unwrap().hash.clone();
    reg.register(make_record(2, None));
    assert_eq!(reg.get(&id).unwrap().hash, before);
}
