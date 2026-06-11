# TODO Phase 3 — Contract Spec

## 7. External schema extraction
- Export semantic_bias_v1.schema.json.
- Document:
  - fields;
  - ordering rules;
  - invariants;
  - version constraints.

## 8. Version sealing enforcement
- BiasVersion enum sealed.
- Unknown version -> hard panic.
- No silent fallback allowed.

## 9. Artifact registry index
- Unified artifact index:
  - id
  - type
  - seed
  - timestamp
- Query API independent of execution layer.
