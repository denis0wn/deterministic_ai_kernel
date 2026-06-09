# Architecture

- `execution` performs runtime work only.
- `scheduler` selects the next step only.
- `worker` owns leases and step execution.
- `event_bus` persists events and semantic artifacts.
- `workflow` defines canonical task classes, step kinds, and contracts.
- `replay` and `snapshot` operate against the persisted event/log state.
- Semantic artifacts are typed and currently limited to `analysis_seed`, `retrieval_result`, and `classification`.
