# Show HN post — draft

**Title (≤80 chars):**
Show HN: We measured it — frontier models fail this money-rounding fix 0/14; our kernel does 14/14

**Body:**

I built a deterministic execution kernel for AI code-fix tasks, and I
brought measurements instead of adjectives.

The setup: a small payment library with a seeded defect — a
`proportional_refund` function that truncates instead of rounding
HALF-UP to the cent (`1.0 × 1/8 = 0.125` must become `0.13`). The
repository's own test suite is the judge.

What I measured on this class (all runs committed in-repo, with the
scripts to re-run them):

- Frontier-class cloud model (unassisted): **0/14** — it returns
  `round(raw*100)/100` (banker's) or `round(raw*100+0.5)/100` (the
  round-vs-floor confusion). Plausible-looking patches; the real tests
  fail.
- Local 12–14B reasoning model (unassisted): **0/14**.
- Local model + the kernel's Layer-1 domain hints: **14/14**, each with
  the correct fix (`float(Decimal(str(raw)).quantize(..., ROUND_HALF_UP))`).

The kernel's point is not that it makes models smarter — it's that it
makes results *provable*:

- Six bounded steps (read → locate → patch → apply → run real tests →
  validate). The model proposes; the kernel disposes.
- Tests run sandboxed (macOS Seatbelt: network denied, writes confined
  to the workspace).
- Every step persists an evidence artifact; every model call is stored
  with its full prompt, full response, and seed.
- Any run seals into a replay capsule and replays deterministically.
- When it can't prove success it says `tests_failed` and stops. It has
  never fabricated a pass in any recorded run — and we ship the failing
  episodes in the same evidence directory.

Not claimed: autonomy, self-improvement, benchmark theater. This is an
execution boundary with receipts — built for teams who must answer
"show me exactly what the model was told, what it answered, and what
ran" to a reviewer or an auditor (EU AI Act Art. 12 record-keeping
lands for standalone high-risk systems in December 2027).

Repo: [link]. Evidence: `analyzer_out/` (every series script included).
Demo: `DEMO_SCRIPT_60S.md` + `demo_60s.cast` (asciinema), or I can fail
live on a call — failure is on-message.

Happy to answer anything about the measurement methodology — the
surprising part was how *stable* the wrong answers are across models.

---

## Presenter notes (not part of the post)

- If asked about determinism claims: temp 0, same prompt → same output
  observed; cross-session artifact identity is NOT claimed (payload
  wording moves outcomes — we documented that openly).
- If asked "why not LangSmith/Langfuse": they watch pipelines; this IS
  the pipeline with proof. Different layer.
- If asked about the loop/self-repair: it exists (Layer 2 POC), and we
  measured that this model class does not exploit located-rung feedback
  (0/5 conversion) — published as negative evidence.
