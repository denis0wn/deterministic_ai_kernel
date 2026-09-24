use deterministic_ai_kernel::kernel_types::{
    AIModality, AIRequest, AIResponse, AITrace, AIWorkerHealth,
};
use deterministic_ai_kernel::workflow::contract::WorkerCapability;

#[test]
fn ai_kernel_types_round_trip_basic_contract() {
    let req = AIRequest {
        request_id: "req-1".into(),
        workflow_id: "wf-1".into(),
        modality: AIModality::Text,
        prompt: "hello".into(),
        model_id: Some("model-x".into()),
        seed: Some(7),
    };

    let resp = AIResponse {
        request_id: req.request_id.clone(),
        output_text: Some("world".into()),
        output_artifact_ids: vec!["artifact-1".into()],
        finished: true,
    };

    let trace = AITrace {
        workflow_id: req.workflow_id.clone(),
        worker_id: "worker-ai".into(),
        request_id: req.request_id.clone(),
        model_id: req.model_id.clone(),
        prompt_hash: "prompt-hash".into(),
        config_hash: "config-hash".into(),
        seed: req.seed,
    };

    assert_eq!(req.modality, AIModality::Text);
    assert_eq!(resp.request_id, "req-1");
    assert_eq!(trace.worker_id, "worker-ai");
    assert_eq!(trace.seed, Some(7));
}

#[test]
fn ai_worker_capability_variant_exists() {
    let capability = WorkerCapability::Ai;
    assert_eq!(capability, WorkerCapability::Ai);
}

#[test]
fn ai_worker_health_states_are_stable() {
    assert_eq!(AIWorkerHealth::Ready, AIWorkerHealth::Ready);
    assert_ne!(AIWorkerHealth::Ready, AIWorkerHealth::Busy);
}
