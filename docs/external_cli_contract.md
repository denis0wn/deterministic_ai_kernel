# External CLI Contract

Status: stable local integration contract for machine-mode CLI usage.

## Stable command set

The following commands are the stable machine-mode CLI surface for external integration:

- `integrity-json`
- `doctor-json`
- `capture-capsule-save <task_id> --json`
- `replay-capsule <task_id> --json`
- `compare-capsules <task_id_a> <task_id_b> --json`

External integrations should treat only this command set as the frozen JSON ABI surface.

## Invocation rules

External callers must invoke these commands as subprocess-style CLI operations and must consume stdout as machine data.

Rules:

- call one command per process execution
- treat stdout as the machine channel
- treat stderr as the human/error channel
- do not parse stderr as machine data
- do not depend on human-mode commands for automation
- do not depend on help text, banners, or free-form output

## Output contract

Each stable machine-mode command guarantees the following on success:

- exactly one stdout write
- exactly one JSON object on stdout
- no additional stdout text before or after the JSON object
- no partial JSON fragments
- no human-readable success banners
- no mixed human/machine output in success path

All successful machine-mode JSON output must follow this pipeline:

`build_report() -> command_report() -> print_json_report()`

Where:

- `build_report()` produces the domain-specific report payload
- `command_report()` wraps the payload into the standard CLI envelope
- `print_json_report()` is the sole stdout serialization boundary

## Error contract

On failure:

- stderr may contain human-readable error text
- stdout must not be treated as a valid JSON ABI payload unless the command completed successfully
- integrations must treat non-zero process exit as failure
- integrations must not merge stdout and stderr before parsing

## Integration invariants

External integrations must assume the following invariants:

- machine-mode and human-mode CLI paths are distinct
- replay-safe automation must depend only on the JSON payload
- logs and human-oriented diagnostics are outside the machine ABI
- the JSON ABI is defined by command identity plus envelope structure, not by surrounding terminal output

## Replay boundary

Replay-compatible external systems must consume only the JSON payload returned by the stable machine-mode commands.

They must not derive state from:

- stderr text
- terminal banners
- help output
- command ordering side effects outside the explicit JSON response

## Compatibility rule

Future CLI work must preserve the behavior of this stable machine-mode command set unless the ABI version is explicitly changed.
