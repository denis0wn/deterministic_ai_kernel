use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::workflow::semantic::bias::BiasVersion;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    SemanticBias,
    ReplayCapsule,
    Snapshot,
    VerificationPlan,
    VerificationVerdict,
}

/// Lightweight index entry. Never stores the artifact payload itself —
/// only the metadata needed to locate or verify it without replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id:            Uuid,
    pub artifact_type: ArtifactType,
    pub seed:          u64,
    pub version:       BiasVersion,
    pub timestamp:     DateTime<Utc>,
    pub parent:        Option<Uuid>,
    /// BLAKE3 digest of the canonical JSON payload (32 bytes, hex-encoded).
    pub hash:          String,
}

impl ArtifactRecord {
    pub fn new(
        artifact_type: ArtifactType,
        seed:          u64,
        version:       BiasVersion,
        parent:        Option<Uuid>,
        payload_bytes: &[u8],
    ) -> Self {
        Self {
            id:            Uuid::new_v4(),
            artifact_type,
            seed,
            version,
            timestamp:     Utc::now(),
            parent,
            hash:          blake3::hash(payload_bytes).to_hex().to_string(),
        }
    }
}

/// In-memory index. Append-only: records are never removed or mutated.
#[derive(Debug, Default)]
pub struct ArtifactRegistry {
    records: Vec<ArtifactRecord>,
}

impl ArtifactRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, record: ArtifactRecord) -> Uuid {
        let id = record.id;
        self.records.push(record);
        id
    }

    pub fn get(&self, id: &Uuid) -> Option<&ArtifactRecord> {
        self.records.iter().find(|r| &r.id == id)
    }

    pub fn find_by_seed(&self, seed: u64) -> Vec<&ArtifactRecord> {
        self.records.iter().filter(|r| r.seed == seed).collect()
    }

    pub fn find_by_type(&self, artifact_type: &ArtifactType) -> Vec<&ArtifactRecord> {
        self.records
            .iter()
            .filter(|r| &r.artifact_type == artifact_type)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
