# CLI JSON ABI Contract

Status: frozen machine-mode ABI.

## Locked commands

The following commands are the only locked JSON ABI commands:

- `integrity-json`
- `doctor-json`
- `capture-capsule-save --json`
- `replay-capsule --json`
- `compare-capsules --json`

## Success-path contract

Each locked command must satisfy all of the following:

- stdout contains exactly one JSON object
- success path performs exactly one stdout write
- success path emits no additional text before or after the JSON object
- success path emits no human-readable banners, debug logs, or partial JSON fragments
- success path does not bypass the shared CLI envelope builder
- success path does not serialize directly

## Required pipeline

All locked commands must follow this structural pattern:

`build_report() -> command_report() -> print_json_report()`

Where:

- `build_report()` constructs the domain-specific report payload
- `command_report()` wraps the payload in the standard CLI envelope
- `print_json_report()` is the single and only stdout serialization boundary

## Serialization boundary

The only approved machine-mode stdout boundary is:

- `src/cli_json.rs`: `print_json_report(report: &Value)`

The only approved shared CLI envelope builder is:

- `src/cli_json.rs`: `command_report(command: &str, report: Value)`

## Error-path rule

Error paths may use `eprintln!`, but they must not emit JSON and must not call `print_json_report()`.

## Human-mode commands

All other CLI commands are human-mode commands.

Human-mode commands may emit free-form text and are not part of the locked JSON ABI.

## Invariant

The CLI is a dual-mode interface:

- machine mode = locked JSON ABI
- human mode = developer/operator UX

These modes must never mix in a single success path.
