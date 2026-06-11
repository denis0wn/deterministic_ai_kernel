# LM Policy Layer Implementation

## Scope
This implementation adds an external policy layer that operates only on confirmed policy surfaces and does not modify execution, replay, or event semantics.[cite:24][cite:26][cite:27][cite:28][cite:30]

## Added modules
- `src/lm_policy_layer/mod.rs`: policy state, normalized policy context, strict surface allowlist, and version record schema.
- `src/lm_policy_layer/lm_studio_client.rs`: request/response adapter and strict output validation.
- `src/lm_policy_layer/policy_applier.rs`: reversible apply engine with confidence gating, version snapshots, and append-only apply log.

## Policy state model
The policy layer represents four confirmed surfaces: model selection, RAM gating, planner heuristics, and env sync. The context builder aggregates manifest state, current env sync status, local model availability, and free memory into a normalized LM input payload.[cite:24][cite:26][cite:27][cite:28]

## Safety behavior
The adapter rejects unsupported surfaces and low-confidence responses. The apply engine writes only policy version artifacts under `policy_versions/` and does not touch execution-core modules, replay modules, or event schema files.[cite:16][cite:22][cite:30]
