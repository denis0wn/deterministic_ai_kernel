use super::bias::SemanticBias;
use crate::workflow::contract::StepKind;

pub struct SeedInterpreter;

impl SeedInterpreter {
    pub fn interpret(seed: u64, domain: &[StepKind]) -> SemanticBias {
        let mut bias = SemanticBias::neutral_for(domain);

        for (idx, kind) in domain.iter().enumerate() {
            let mixed = mix(seed, idx as u64);
            let weight = 1.0 + ((mixed % 10_000) as f64 / 10_000.0);
            bias.weights.insert(format!("{:?}", kind), weight);
        }

        let mut ranked: Vec<(u64, StepKind)> = domain
            .iter()
            .cloned()
            .enumerate()
            .map(|(idx, kind)| (mix(seed, idx as u64), kind))
            .collect();

        ranked.sort_by_key(|(rank, _)| *rank);
        bias.preferred = ranked.into_iter().map(|(_, kind)| kind).collect();

        bias
    }
}

fn mix(seed: u64, salt: u64) -> u64 {
    let mut x = seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}
