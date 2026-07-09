# LLM preflight checklist

Before any model integration, verify the runtime environment in isolation.

## Python runtime

- confirm Python version
- confirm selected interpreter path
- confirm virtual environment is active and reproducible

## Python package state

- confirm `transformers` version
- confirm any required tokenizer packages
- confirm backend-specific dependencies

## Backend availability

Verify at least one intended backend is available:

- MLX availability
- or PyTorch availability

Record:
- backend name
- backend version
- device visibility

## Tokenizer validation

- tokenizer imports successfully
- tokenizer loads from target model id/path
- encode/decode roundtrip works on a minimal prompt

## Model weights validation

- model weights can be discovered
- model weights load without kernel involvement
- model metadata can be inspected
- model hash can be captured

## Minimal inference probe

Run one minimal isolated inference outside kernel path:

Input:
- tiny prompt

Expected:
- process returns success/failure cleanly
- generated text is non-empty on success
- latency and metadata are captured
- failure path is structured and non-fatal

## Failure policy

If any preflight step fails:
- do not connect model to kernel
- keep worker isolated
- fix environment first
