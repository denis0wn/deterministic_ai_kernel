use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactType {
    AnalysisSeed,
    RetrievalResult,
    Classification,
    SemanticBiasV1,
    PipelineStep,
    PipelineReport,
    PrimitiveResultV1,
    EmbeddingResult,
    FinalAnswer,
}

impl ArtifactType {
    pub fn all() -> &'static [ArtifactType] {
        &[
            ArtifactType::AnalysisSeed,
            ArtifactType::RetrievalResult,
            ArtifactType::Classification,
            ArtifactType::SemanticBiasV1,
            ArtifactType::PipelineStep,
            ArtifactType::PipelineReport,
            ArtifactType::PrimitiveResultV1,
            ArtifactType::EmbeddingResult,
            ArtifactType::FinalAnswer,
        ]
    }
}

impl fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ArtifactType::AnalysisSeed => "analysis_seed",
            ArtifactType::RetrievalResult => "retrieval_result",
            ArtifactType::Classification => "classification",
            ArtifactType::SemanticBiasV1 => "semantic_bias_v1",
            ArtifactType::PipelineStep => "pipeline_step",
            ArtifactType::PipelineReport => "pipeline_report",
            ArtifactType::PrimitiveResultV1 => "primitive_result_v1",
            ArtifactType::EmbeddingResult => "embedding_result",
            ArtifactType::FinalAnswer => "final_answer",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for ArtifactType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "analysis_seed" => Ok(ArtifactType::AnalysisSeed),
            "retrieval_result" => Ok(ArtifactType::RetrievalResult),
            "classification" => Ok(ArtifactType::Classification),
            "semantic_bias_v1" => Ok(ArtifactType::SemanticBiasV1),
            "pipeline_step" => Ok(ArtifactType::PipelineStep),
            "pipeline_report" => Ok(ArtifactType::PipelineReport),
            "primitive_result_v1" => Ok(ArtifactType::PrimitiveResultV1),
            "embedding_result" => Ok(ArtifactType::EmbeddingResult),
            "final_answer" => Ok(ArtifactType::FinalAnswer),
            _ => Err(format!("Unknown ArtifactType: {}", s)),
        }
    }
}
