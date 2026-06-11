# TODO Phase 2 — CLI Reduction

## 4. Canonical CLI surface reduction

Current issue: multiple entry points exist:
- bias-explain
- latest-bias-artifact
- snapshot
- restore
- semantic-artifacts CLI variants

Tasks:
- Define canonical CLI map:
  - replay
  - artifact latest
  - bias explain
- Deprecate all alternative commands.
- Ensure single entry per semantic action.

## 5. Remove duplicated semantic entry points
- Audit CLI for multiple paths to same data.
- Remove redundant explain implementations.
- Ensure one explain pipeline only.

## 6. Enforce module boundary rules
- Disallow semantic -> CLI imports.
- Disallow planner -> CLI coupling.
- Enforce: CLI = thin shell only.
