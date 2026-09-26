# Security Policy

This project is an execution boundary for AI-driven code changes; its
security properties are the product.

## What the kernel guarantees

- **Fail-closed everywhere.** Missing config, dead endpoints, malformed
  patches, failed tests — all stop the task; nothing is silently
  approximated.
- **No fabricated success.** A task is reported complete only when the
  repository's own test suite passes in a sandboxed process. The honesty
  invariant is enforced in code and covered by tests, including a
  reproduced-and-fixed exit-code spoofing vector (`os._exit(0)` in
  model-influenced code).
- **Sandboxed test execution.** macOS: Seatbelt (network denied, writes
  confined to the workspace, credential stores unreadable). Linux:
  bubblewrap with the same policy. The report records which backend
  confined the run — an unsandboxed run says `none`, never overclaims.
- **Workspace confinement.** All file access goes through canonicalized
  path resolution; absolute paths outside the authorized workspace are
  rejected.
- **Full evidence.** Every model call is persisted with its prompt,
  response, and seed; every run replays against its evidence chain.

Security review artifacts live in-repo: `LAYER2_SECURITY_REVIEW.md` and
the commit history reference the reproduced findings and their fixes.

## Reporting a vulnerability

Do NOT open a public issue. Contact the maintainer directly (see the
repository owner's profile). Include: the affected commit, a reproduction
or a code path, and your assessment of reachability. We answer within
72 hours.

## Scope

In scope: the execution pipeline (`src/effects.rs`, `src/execution/`),
tool layer (`src/tools/`), event/evidence persistence (`src/event_bus.rs`,
`src/providers/storage.rs`), and the TUI/runtime spawn paths.
Out of scope: the bundled demo fixtures (`analyzer_examples/`) — they are
intentionally buggy teaching targets, not production code.
