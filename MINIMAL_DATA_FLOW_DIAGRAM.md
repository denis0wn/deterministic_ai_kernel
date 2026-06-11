# Minimal Data Flow Diagram

## Purpose
This document defines the smallest passive data flow for the learning layer MVP.

## Flow

```text
Deterministic Kernel Outputs
  ├─ capsules
  ├─ traces
  ├─ evaluations
  └─ execution outcomes
          │
          │ read-only
          ▼
Learning Layer Manifest Builder
  ├─ dataset manifest generation
  ├─ registry record generation
  ├─ evaluation stub record generation
  └─ approval record generation
          │
          ▼
Passive Filesystem Storage
  ├─ learning/datasets/
  ├─ learning/adapters/
  ├─ learning/evaluations/
  ├─ learning/approvals/
  └─ learning/routing/ (metadata only, inactive)
```

## Boundary interpretation
- Kernel artifacts are inputs only.[conversation_history:1][cite:84]
- Learning-layer outputs are passive metadata artifacts only.[cite:84][cite:85]
- No training, no adapter activation, no routing mutation, and no runtime model changes occur in this flow.[cite:85]
