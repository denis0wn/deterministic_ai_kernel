# LLM worker boundary design

## Goal

Prepare an isolated LLM integration boundary without coupling model runtime into kernel execution.

Current target shape:

Kernel
 |
 IPC/API boundary
 |
 LLM Worker
 |
 Gemma runtime

## Design constraints

- no direct model coupling inside kernel
- no kernel crash on LLM unavailability
- deterministic request/response envelope
- scheduler / replay / snapshot compatibility preserved
- future backend details hidden behind worker boundary

## Request contract

Kernel sends a single request envelope:

- `request_id`: stable request identifier
- `prompt`: final prompt text
- `model_parameters`:
  - model id / alias
  - temperature
  - max tokens
  - top_p
  - stop sequences
  - optional deterministic seed

Optional future fields:
- timeout budget
- trace metadata
- replay correlation id

## Response contract

Worker returns one response envelope:

- `request_id`
- `success`: boolean
- `generated_text`: final text on success
- `runtime_metadata`:
  - model id
  - backend name
  - prompt token count
  - generated token count
  - latency_ms
  - load state / warm state
  - model hash
  - prompt hash

## Failure contract

Worker failure must be reported as data, never as a kernel crash.

Failure response shape:
- `request_id`
- `success=false`
- `error_code`
- `error_message`
- `runtime_metadata`

Examples:
- `LLM_UNAVAILABLE`
- `MODEL_NOT_LOADED`
- `TOKENIZER_LOAD_FAILED`
- `INFERENCE_FAILED`
- `REQUEST_TIMEOUT`

## Isolation rule

Kernel owns orchestration only.
Worker owns model runtime only.

Kernel must not depend on:
- transformers internals
- MLX/PyTorch details
- tokenizer internals
- model loading lifecycle

That responsibility stays inside the worker.

## Safe evolution path

1. freeze request/response schema
2. define transport boundary
3. implement stub worker
4. add health/preflight path
5. only then connect Gemma runtime
