# Business Analysis — deterministic_ai_kernel pilot business

Date: 2026-09-26. Written as the honest deputy's memo: strengths,
weaknesses (no varnish), what the market actually needs, price analysis,
contract analysis. Sources at the end; [confirmed] = verified in-repo or
primary source, [likely] = secondary sources, [assumption] = untested.

## 1. What the project can do (confirmed, measured)

- Run an AI code-fix task through a bounded pipeline with the repository's
  own tests as the judge; persist every prompt, response, seed, and test
  report; replay any run deterministically against its evidence chain.
- Measured on the reference defect class: unassisted frontier-class cloud
  model 0/14; kernel + Layer-1 hints 14/14; honest failure on every wrong
  attempt, zero fabricated successes across 70+ live episodes.
- Security reviewed; test execution sandboxed (network denied, workspace
  confined) on macOS; 933 automated tests green; CI green on every merge.

## 2. Minuses (the part a good deputy does not hide)

| # | Weakness | Severity for sales |
|---|---|---|
| W1 | ~~**One defect class proven.**~~ **CLOSED 2026-09-26:** three classes measured on the sanctioned local model — money-rounding (0/14 plain → 14/14 hints), business-days (0/8 → 3/8 after the fixture's own test hole was found and closed *by reading the evidence*), null-safety (8/8 both). Bonus finding: the evidence chain exposed a hole in our own test suite — the machinery measures its own judges. | High — mitigated by selling the evaluation, not the claim |
| W2 | **Model-dependent.** Without hints every model tested fails the class (0/14). The kernel doesn't make models smarter; hints are hand-written recipes per defect class. | High — the Layer-1 hint library becomes the real asset to build per client |
| W3 | **Layer-2 self-repair doesn't convert** — measured on TWO models now: gemma4 0/5, Ministral 0/8 (2026-09-26). The failure is total: at temp 0 the model emits the *identical* patch on every attempt (evidence: same replacement 3× per task). The class needs recipe knowledge (hints), not a verification signal. Side yield: the runs exposed a futility-stop bug — prose drift in the patch's `reason` field defeated the raw-JSON identical check; now hashed on the semantic triple only (test added). | Medium — we simply don't claim it |
| W4 | **One operator.** Bus factor, response times, enterprise procurement optics. | Medium — standard for a pilot stage; disclose |
| W5 | ~~**macOS-only sandbox** (Seatbelt).~~ **CLOSED 2026-09-26:** Linux covered via bubblewrap (ro-bind /, rw workspace, `--unshare-net`, credential dirs masked); the report records `sandbox_backend` honestly (`seatbelt`/`bwrap`/`none`). | Medium — matters only for self-hosted Linux shops |
| W6 | **No managed offering.** Self-hosted only; no SaaS, no SLAs, no support desk. | Medium — fine for pilot, blocks scale-up later |
| W7 | **Zero community proof.** Public repo as of today; no stars, no third-party usage yet. | High for inbound; irrelevant for outbound pilots |

## 3. What people actually need (market pull)

- **Proof for sign-off.** Credo AI's 2026 governance survey: ~60% of
  enterprises scale AI, ~4% govern it [likely: vendor-reported]. The
  governance platforms (Credo, Monitaur, Holistic) sell inventory/policies
  at 5–6 figures/yr — none can replay an execution. Our layer is empty
  [confirmed by category review].
- **AI Act record-keeping.** Art. 12 automatic logging for high-risk
  systems, deadline Dec 2027 (standalone) [confirmed]. Buyers need
  automatic, replayable run logs — which is precisely our evidence pack.
- **Agencies need proof-of-work** to get client sign-off on AI-assisted
  delivery [assumption — from agency-model familiarity, not a survey].

## 4. Price analysis (2026 evidence)

| Comparable | Price | Source |
|---|---|---|
| AI agency pilot/MVP | $5k–$15k | AGIX 2026 guide |
| 4-week AI PoC, fixed scope | low-to-mid five figures | SumatoSoft |
| Enterprise paid pilot w/ credit | $25k–$50k for 90 days, credited to annual | getmonetizely (Snowflake pattern) |
| Independent consultant pilot | $10k–$25k flat, 6 weeks | aiessentials.us 2026 |
| Boutique AI program | $35k–$150k | bosio.digital 2026 |
| Governance platforms (annual) | tens of thousands – low six figures | domo.com 2026 |

Pilots with predefined success criteria convert to paid contracts 3.2×
more often than open-ended ones [likely: Forrester via getmonetizely].

**Our price point (recommended):** the kernel today is a
consulting-delivered pilot, not a product SKU. Position at the
independent/boutique band with the credit mechanism:

- **Week-0 proof competition: free** (our fixture or theirs).
- **4-week pilot: $15,000 fixed** — mid-band for a 4-week PoC, credible
  against $5k (freelancer floor) and $25k+ (enterprise tooling).
- **100% credited** against the year-1 license if they convert
  (Snowflake pattern).
- **Year-1 license: $48,000/yr** — self-hosted kernel + hint-library
  updates + evidence-pack support; deliberately under the governance
  platforms' band, positioned as the execution-proof layer *under* them.

All numbers marked [assumption] until validated by the first three
conversations; adjust on evidence, not on feelings.

## 5. Contract analysis — what these deals look like

Standard instrument: a short **Pilot Agreement** (or MSA + one-page SOW).
Norms for this class of deal:

- Fixed scope, fixed fee, fixed duration, **predefined success criteria**
  (the Forrester conversion finding).
- IP: client owns their data and the evidence packs; we retain the kernel
  and the hint-library IP; client gets a perpetual internal-use license to
  whatever config/hints we build for them.
- Confidentiality mutual; our evidence packs may contain their file
  contents — data handling must be explicit.
- No warranty on model output (the kernel never claims success without
  proof — the contract mirrors that honesty: we warrant the *evidence*,
  not the fix).
- Liability capped at fees paid; no consequential damages.
- Either party terminates for convenience with 2 weeks' notice; fees for
  work performed are non-refundable.
- Independent contractor status; no exclusivity.

The ready-to-sign template: `docs/PILOT_AGREEMENT.md`.

## Sources

- https://agixtech.com/how-much-does-it-cost-to-hire-an-ai-agency-in-2026-the-ultimate-pricing-guide
- https://sumatosoft.com/services/ai-proof-of-concept-development
- https://www.getmonetizely.com/articles/how-to-structure-enterprise-pilot-program-pricing-effective-proof-of-concept-strategies
- https://aiessentials.us/blog/how-much-does-it-cost-to-hire-an-ai-consultant-for-my-small
- https://bosio.digital/articles/ai-consulting-cost-guide
- https://www.domo.com/learn/article/ai-governance-tools
- https://www.credo.ai
