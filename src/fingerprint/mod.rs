use serde_json::Value;
use std::collections::BTreeMap;

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut blocks = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    blocks.push(0x80);
    while (blocks.len() + 8) % 64 != 0 {
        blocks.push(0);
    }
    blocks.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in blocks.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_val = h[7];

        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h_val
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_val = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_val);
    }

    let mut result = [0u8; 32];
    for i in 0..8 {
        result[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    result
}

pub fn generate_fingerprint(payload: &BTreeMap<String, Value>) -> String {
    let serialized = serde_json::to_string(payload).unwrap_or_default();
    let input = format!("v1:{}", serialized);
    let hash_result = sha256(input.as_bytes());
    hash_result.iter().map(|b| format!("{:02x}", b)).collect()
}

// ── Deterministic fingerprint types ───────────────────────────────────────────

/// Canonical fingerprint for a task submission.
/// Same (normalized_input, environment, versions) MUST produce identical output.
pub struct TaskFingerprint<'a> {
    pub normalized_input: &'a str,
    pub environment_fingerprint: &'a str,
    pub manifest_version: &'a str,
    pub kernel_version: &'a str,
}

impl<'a> TaskFingerprint<'a> {
    pub fn compute(&self) -> String {
        let mut map = BTreeMap::new();
        map.insert(
            "normalized_input".to_string(),
            Value::String(self.normalized_input.to_string()),
        );
        map.insert(
            "environment_fingerprint".to_string(),
            Value::String(self.environment_fingerprint.to_string()),
        );
        map.insert(
            "manifest_version".to_string(),
            Value::String(self.manifest_version.to_string()),
        );
        map.insert(
            "kernel_version".to_string(),
            Value::String(self.kernel_version.to_string()),
        );
        generate_fingerprint(&map)
    }
}

/// Canonical fingerprint for a compiled plan.
pub struct PlanFingerprint<'a> {
    pub task_fingerprint: &'a str,
    /// Ordered list of step identifiers (e.g. step kind + index).
    pub ordered_steps: &'a [String],
    pub planner_version: &'a str,
}

impl<'a> PlanFingerprint<'a> {
    pub fn compute(&self) -> String {
        let steps_val: Vec<Value> = self
            .ordered_steps
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect();
        let mut map = BTreeMap::new();
        map.insert(
            "task_fingerprint".to_string(),
            Value::String(self.task_fingerprint.to_string()),
        );
        map.insert("ordered_steps".to_string(), Value::Array(steps_val));
        map.insert(
            "planner_version".to_string(),
            Value::String(self.planner_version.to_string()),
        );
        generate_fingerprint(&map)
    }
}

/// Canonical fingerprint for a single primitive execution step.
pub struct StepFingerprint<'a> {
    pub primitive_type: &'a str,
    pub primitive_version: &'a str,
    pub canonical_input: &'a Value,
    pub dependency_hash: Option<&'a str>,
    pub environment_fingerprint: &'a str,
}

impl<'a> StepFingerprint<'a> {
    pub fn compute(&self) -> String {
        let mut map = BTreeMap::new();
        map.insert(
            "primitive_type".to_string(),
            Value::String(self.primitive_type.to_string()),
        );
        map.insert(
            "primitive_version".to_string(),
            Value::String(self.primitive_version.to_string()),
        );
        map.insert("canonical_input".to_string(), self.canonical_input.clone());
        map.insert(
            "dependency_hash".to_string(),
            match self.dependency_hash {
                Some(h) => Value::String(h.to_string()),
                None => Value::Null,
            },
        );
        map.insert(
            "environment_fingerprint".to_string(),
            Value::String(self.environment_fingerprint.to_string()),
        );
        generate_fingerprint(&map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_fingerprint_equivalence() {
        let mut map1 = BTreeMap::new();
        map1.insert("a".to_string(), serde_json::json!(1));
        map1.insert("b".to_string(), serde_json::json!(2));

        let mut map2 = BTreeMap::new();
        map2.insert("b".to_string(), serde_json::json!(2));
        map2.insert("a".to_string(), serde_json::json!(1));

        let hash1 = generate_fingerprint(&map1);
        let hash2 = generate_fingerprint(&map2);

        assert_eq!(hash1, hash2);
    }
}
