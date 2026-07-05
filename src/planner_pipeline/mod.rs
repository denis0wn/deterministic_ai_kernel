use anyhow::Result;
use crate::semantic_bias::BiasVersion;

pub struct PipelineContext {
    pub seed: u64,
    pub bias_version: BiasVersion,
}

pub trait PipelineStage {
    type Input;
    type Output;
    fn run(&self, input: Self::Input, ctx: &PipelineContext) -> Result<Self::Output>;
}

pub struct RawInput {
    pub payload: String,
}

pub struct IntermediateRepresentation {
    pub steps: Vec<String>,
}

pub struct Plan {
    pub id: String,
    pub steps: Vec<String>,
    pub seed: u64,
}
