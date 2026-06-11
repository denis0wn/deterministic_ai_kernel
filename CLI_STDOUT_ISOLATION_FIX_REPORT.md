# CLI Stdout Isolation Fix Report

## What broke
`capture-capsule-save --json` failed because the test could not parse the command output as a single JSON document. The failure mode was `trailing characters`, which means the JSON payload was followed by extra output.[cite:46]

## Why it broke
The CLI path used ordinary stdout printing in a binary that also emits warnings and other status output elsewhere. In JSON mode, that breaks the single-document contract even when the JSON envelope itself is structurally correct.[cite:46][cite:53]

## Minimal fix
A dedicated JSON-mode emitter was introduced in `src/main.rs` and wired into the `capture-capsule-save --json` path. The emission now goes through a buffered stdout writer with an explicit flush, and the change does not touch capsule logic, replay logic, or policy logic.

## Core untouched
Execution core, replay system, event model, and policy layer behavior were not modified by this fix.

## Validation plan
Re-run only `capture_capsule_save_cli_json` and confirm:
- `serde_json::from_str(out)` succeeds
- no trailing bytes remain
- the output contains exactly one valid JSON document
