# Policy Surface Map

## Scope
This map lists only policy surfaces confirmed in code and configuration. It excludes hypothesized surfaces and excludes execution-core internals that are not present in the inspected files.[cite:24][cite:26][cite:27][cite:28]

## Confirmed policy surfaces

| Surface | Location | What is tunable | Evidence |
|---|---|---|---|
| Role-to-model selection | `src/model_manifest.rs` + `config/model_manifest.json` | Enabled model choice by role and priority ordering | `best_enabled_model_for_role()` selects the lowest `priority` enabled model for a role; config contains role/priority/enabled fields.[cite:27][cite:28] |
| Memory threshold gate | `src/model_manifest.rs` + `src/lm_control/policy.rs` | RAM threshold per `ram_class` and model switch gating | `threshold_gb_for_ram_class()` maps `light/medium/heavy` to 2/6/10 GB; `switch_plan()` binds the selected model to that threshold.[cite:26][cite:27] |
| Switch decision execution gate | `src/lm_control.rs` | Safe switch vs dry run; availability and free-memory checks | `safe_switch()` and `dry_run_switch()` validate free memory and local model availability before applying sync.[cite:24][cite:26] |
| Environment sync target | `src/model_manifest.rs` | Which `.env` keys are written for each role | `env_key_for_role()` and `sync_env_for_role()` map roles to env keys and update `.env`.[cite:27] |
| Planner fallback language handling | `src/workflow/planner.rs` | Fallback normalization heuristics | Search results show `fallback` handling in planner normalization rules and tests.[cite:25] |
| Step priority / canonical flow selection | `src/workflow/contract.rs`, `src/scheduler.rs`, `docs/spec/workflow_abi_v1.md` | Canonical flow selection and step ordering | The ABI doc requires scheduler to read `task_class`, map canonical flows, and not infer semantics from `task_id`; scheduler slug ordering is explicit in code.[cite:21][cite:22] |

## External adaptive boundary
The confirmed adaptation boundary is the LM control / manifest layer, not the execution core. The code already exposes role-based model selection, memory gating, and environment sync as the main tunable surfaces.[cite:24][cite:26][cite:27][cite:28]

## Non-surfaces
The inspected code does **not** confirm any policy surface inside replay logic, event schema, or execution core that is safe for external adaptation. Those areas should remain frozen unless a later audit proves a contract-level extension point.[cite:16][cite:18][cite:19][cite:22]
