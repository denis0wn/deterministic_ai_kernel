# Verification artifacts

## verification_plan.json

`verification_plan.json` is the deterministic execution plan for the verification graph.

It records:
- selected pipeline
- ordered nodes
- execution order id
- graph schema version
- environment identity
- plan hash / plan id

Its purpose is reuse safety. A later run may compare against this artifact to determine whether a previous verification result is still compatible with the current graph and environment.

## verification_verdict.json

`verification_verdict.json` is the machine-readable result of a verification run.

It records:
- overall ok / failure
- status
- artifact_error when applicable
- environment details
- selected pipeline
- node execution results
- timing / metadata needed for diagnostics

Its purpose is to distinguish valid verification success, reusable invalidation, and actual execution failures.

## invalid_reuse

`invalid_reuse` means the previous verification artifact cannot be safely reused.

This is not the same as a kernel failure.

Typical causes:
- malformed or corrupted reuse artifact
- schema mismatch
- missing execution identity
- environment fingerprint mismatch
- execution order mismatch

`invalid_reuse` means: discard reuse assumption and rerun verification from a valid baseline.

## Negative test cases

Expected negative test coverage includes:
- missing environment identity
- invalid environment fingerprint format
- missing execution_order_id
- corrupted ordered_nodes
- graph_schema_version mismatch

These tests prove artifact validation executes before reuse comparison.

## Failure classes

### Artifact corruption

Artifact corruption means the saved verification artifact is malformed or incomplete.

Examples:
- missing fields
- bad fingerprint format
- damaged ordered_nodes structure
- duplicate ordered node keys

Result:
- `status=invalid_reuse`
- artifact must not be trusted

### Environment drift

Environment drift means the artifact was valid when produced, but current execution environment no longer matches.

Examples:
- different environment fingerprint
- changed toolchain identity
- execution order no longer matches current graph

Result:
- `status=invalid_reuse`
- rerun verification under current environment

### Real kernel failure

A real kernel failure means verification nodes executed and at least one actual runtime or contract check failed.

Examples:
- failed verification node
- contract regression
- scheduler invariant failure
- runtime/preflight execution failure

Result:
- not an artifact reuse problem
- requires code or runtime fix
