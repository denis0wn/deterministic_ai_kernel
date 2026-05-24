use crate::event_bus::EventBus;
use serde_json::json;

pub struct ExecutionEngine {
    bus: EventBus,
}

impl ExecutionEngine {
    pub fn new(bus: EventBus) -> Self {
        Self { bus }
    }

    pub fn run(&self, task_id: &str) {
        for i in 0..3 {
            let step_id = format!("step_{i}");
            let effect_id = format!("{task_id}/{step_id}/dispatch");

            let terminal_event = if i == 1 {
                (
                    "STEP_FAILED".to_string(),
                    json!({"effect_id": effect_id.clone(), "status":"error", "reason":"simulated failure"}),
                )
            } else {
                (
                    "STEP_COMPLETED".to_string(),
                    json!({"effect_id": effect_id.clone(), "status":"ok"}),
                )
            };

            let events = vec![
                ("LEASE_ACQUIRED".to_string(), json!({"worker":"kernel"})),
                (
                    "EFFECT_RESERVED".to_string(),
                    json!({"effect_id": effect_id.clone()}),
                ),
                ("STEP_DISPATCHED".to_string(), json!({"agent":"generic"})),
                terminal_event,
            ];

            self.bus
                .commit_causal_unit(task_id, &step_id, events)
                .unwrap();
        }

        self.bus
            .append_event(task_id, None, "DONE", &json!({}))
            .unwrap();
    }
}
