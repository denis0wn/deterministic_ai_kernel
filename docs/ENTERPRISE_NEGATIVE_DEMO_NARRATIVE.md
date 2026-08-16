# Enterprise Demo Narrative — The Negative Outcome We Show On Purpose

Audience: CTO / Head of Engineering / Risk lead.
Companion documents: ENTERPRISE_PILOT_ONE_PAGER.md,
PILOT_SCOPE_AND_LIMITATIONS.md, PILOT_OPS_RUNBOOK.md,
docs/PILOT_METRICS_V1.md.

## The pitch in one paragraph

Most AI-coding demos show you the one seed that worked. We show you the
run where the model was wrong — and demonstrate what the system does
about it. On 2026-08-16 we executed a real remediation attempt with a
real local model (gemma4-reasoning over MLX) against a synthetic fintech
fixture in an isolated workspace. The model proposed a plausible but
incorrect Python fix. The system applied it through the only authorized
boundary, ran the real tests, observed exit code 1, refused to record
success, and the independent evidence-chain verifier reported
`chain_incomplete` — exactly the status a truthful system must report.
That refusal, with full tamper-evident evidence, is the product.

## What the audience sees (10 minutes)

1. **Read-only analysis** — the analyzer inventories the module, reports
   3 candidate findings with snapshot hashes and documented
   false-positive/false-negative profiles. Nothing is claimed proven.
2. **Human gate** — one finding is approved by a named reviewer; the
   approval is a hash-signed artifact bound to the exact contract and
   snapshot. Approving a finding without a reproducible test is refused
   unless an explicit, permanently recorded override is used.
3. **Attempt 1: the door is locked** — the executor refuses to mutate
   anything without an authorized workspace. No configuration, no
   effects. (Shown live from the attempt log.)
4. **Attempt 3: the model errs honestly** — the patch passes structural
   checks, is applied inside the isolated workspace only, and the
   executor's pre-image hash matches the analyzer's snapshot hash byte
   for byte (cross-boundary continuity). Then the real tests fail. The
   kernel logs «no fabricated success».
5. **Independent verification** — the chain verifier recomputes every
   hash from bytes and reports `chain_incomplete`: integrity links
   verified, passing-test link absent. Nobody relabels it.

## The three takeaways to land

- **Fail-safe over wishful:** a wrong model output becomes a documented
  failed attempt, never a fake success. The gates that stopped it are
  kernel-owned, not prompt-based.
- **Evidence over narrative:** every stage leaves a hash-signed artifact
  (manifest, decision, work order, patch pre/post hashes, event log,
  chain report). An auditor can replay the story from bytes alone.
- **Honest metrics:** success rate is computed over ALL attempts,
  including this one. The pilot metrics schema has a `fake_success`
  counter that must stay zero — its only purpose is to be impossible to
  fill honestly.

## Objection handling (say these, verbatim where quoted)

- «So the AI failed?» — Yes. The model proposed an incorrect change;
  the system detected it through real tests and refused to present it
  as success. The pilot's job is to make that outcome cheap, visible,
  and impossible to spin.
- «What is the success rate then?» — Over all attempts in this run:
  0/3 remediation successes; 3/3 fail-safe behaviors. The rate we sell
  is measured over every attempt, never over the best seed.
- «Can it ever claim something is safe?» — No. The verifier speaks only
  about evidence-chain consistency. It does not certify correctness, and
  the pilot documentation says so in writing.
- «Why not retry until it passes?» — Seed-hunting is prohibited by the
  pilot's own attempt-discipline rules. Further attempts happen only
  inside a pre-registered model-quality experiment with a declared
  budget, and every attempt is published.

## What we never say

- No «the AI guarantees …», no «certified absence of errors», no «the
  fix is verified correct», no «hallucinations are eliminated».
- No presenting the simulated fixtures as real executor runs — they are
  labeled SIMULATED everywhere they appear.
- No publishing an attempt list with the failures removed.

## Closing line

> We are not selling a machine that is always right. We are selling a
> system that cannot pretend to be right — with the evidence trail to
> prove it, on a day when the model was wrong.
