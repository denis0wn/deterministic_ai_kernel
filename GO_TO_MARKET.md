# Go-To-Market — deterministic_ai_kernel

Status: September 2026. Grounded in measured evidence (the repo's own
`analyzer_out/` series) and cited market research. Claims are marked
[confirmed] (verified in-repo or primary source) / [likely] (secondary
sources) / [assumption] (needs a test).

## 1. Market map — where this product sits

Three adjacent markets, none of which owns our layer:

**A. LLM observability/eval** (LangSmith, Langfuse, Braintrust, Arize).
$2.69B in 2026 → $9.26B by 2030 (36% CAGR) [confirmed: Business Research
Company via MarkTechPost, Aug 2026]. They sell traces and dashboards —
they *watch* pipelines. Nobody in this segment offers deterministic replay
or proof artifacts.

**B. AI governance/compliance** (Credo AI — Forrester Wave Leader Q3'25,
Holistic AI, Monitaur, OneTrust, IBM watsonx.governance). Enterprise,
"contact us" pricing, tens of thousands to low six figures per year
[confirmed: domo.com comparison, Willow 2026 review]. They govern AI
*systems as registered objects*: inventory, policies, risk workflows.
None governs *execution* — nobody can replay a run and prove what
happened. EU AI Act Art. 12 record-keeping for high-risk systems:
standalone systems' deadline moved to **Dec 2, 2027** (Digital Omnibus,
law of 2026-06-29) [confirmed: Latham&Wilkins, JDSupra]; 78% of
organizations had not started compliance work as of April 2026 [likely:
RAIL]. The build-out window is now.

**C. AI coding/dev tools** — huge, noisy, zero trust layer. The
independent dev-tool trust guidance (Evil Martians, 2026) reads like our
design doc: "never execute model output directly; validate and constrain"
[confirmed].

**Our layer (empty):** execution-level proof. Not watching, not policies —
a bounded pipeline where every run is replayable and carries a verifiable
evidence chain.

## 2. The one story that sells

Measured, reproducible, and in-repo:

> Even a frontier-class cloud model fails our reference defect class
> (money rounding) **0/14 unassisted**. The kernel with its Layer-1 hint
> layer completes **14/14**, with the same fix verified by the real test
> suite — and when it fails, it says so and never fabricates success.

Evidence: `analyzer_out/mq_northpay_*` series (committed, re-runnable
scripts included). This is the entire wedge: we don't claim the model is
good — we prove the pipeline is trustworthy.

## 3. ICP (who buys first)

1. **Platform/eng leads at fintech, healthcare, insurance** running or
   piloting LLM code-fix/migration automation, who must answer auditors
   or internal review boards. Pain: "we can't show what the model did."
2. **AI-forward agencies/consultancies** doing client migrations, who
   need proof-of-work artifacts to get sign-off (and get paid).
3. **Compliance-adjacent eng teams** preparing for EU AI Act Art. 12
   record-keeping (Dec 2027) who need automatic, replayable run logs.

Anti-ICP: teams wanting an agent that "just fixes things" (we are not
that, and saying so is the brand).

## 4. Motion — founder-led, evidence-first (no paid ads at launch)

**Phase 0 — proof assets (days, done):** `PILOT.md`, `PILOT_OFFER.md`,
`DEMO_SCRIPT_60S.md`, committed evidence series. Remaining: one 60-second
terminal video of the demo script (record it in one take; failure on
camera is on-message).

**Phase 1 — story launch (week 1):**
- Blog post + Show HN: "We measured it: frontier models fail this
  money-rounding fix 0/14. Our kernel doesn't." Link the committed
  evidence, not adjectives. (Show HN guidance: technical specifics in
  comments, terminal recordings over screenshots [confirmed: LaunchPact
  2026 dev-tools guide].)
- LinkedIn/X version of the same for the compliance audience
  (Art. 12 framing).
- Repo visibility decision: the motion works with a private repo +
  evidence packs; going public accelerates trust but exposes the code.
  [assumption: public-with-pilot-launch is the stronger play; user's call.]

**Phase 2 — targeted outbound (weeks 2–6):** 30–50 named companies
(regulated + agencies), short note: the 0/14→14/14 result + the free
week-0 proof competition on *their* task class. The offer sells the
evaluation, not the product — lowest possible commitment.

**Phase 3 — convert (weeks 4–10):** 2–3 paid 4-week pilots → written case
studies with evidence packs → price the annual from measured pilot value.

## 5. Pricing shape (evidence-based)

Governance platforms price at 5–6 figures/year [confirmed above];
consulting-adjacent 4-week technical pilots commonly land $10–25k
[assumption — validate on the first three conversations]. Structure:

- Week 0 proof: free.
- Pilot: fixed fee, 4 weeks, one task class, evidence packs included.
- Post-pilot: annual license (self-hosted kernel + hint-library updates +
  evidence-pack support).

## 6. What kills launches like this (and our countermeasures)

| Killer | Countermeasure |
|---|---|
| Launching a repo without the story | Lead with the measured result, repo second |
| Leading with tech, not pain | The pain is "you can't show what the model did" |
| No proof assets | Every claim links a committed, re-runnable artifact |
| Vague ICP | Phase 2 names 30–50 companies, not "developers" |
| Overselling ("autonomous agent") | Explicit non-claims section in the offer |

## 7. Honest constraints

- One-person operation: the motion above is sized for that (no ads, no
  conference circuit).
- One defect class is proven (NorthPay money-rounding). The pilot pitch
  deliberately sells the *evaluation on the client's class* — the product
  is the proof machinery, not the fixture.
- R3 pilot delivery is the entire next milestone; everything else waits.

## Sources

- https://www.marktechpost.com/2026/08/09/top-llm-observability-and-evaluation-platforms-in-2026-langfuse-langsmith-braintrust-arize-and-more-compared
- https://www.domo.com/learn/article/ai-governance-tools
- https://withwillow.ai/blog/6-best-ai-governance-platforms-for-enterprise-compliance-2026
- https://www.jdsupra.com/legalnews/ai-act-update-eu-resolves-to-change-9734656
- https://www.jamf.com/blog/eu-ai-act-2026-deadlines-it-compliance-guide
- https://responsibleailabs.ai/knowledge-hub/articles/eu-ai-act-august-2026-compliance
- https://www.launchpact.io/product-hunt-launch/developer-tools
- https://evilmartians.com/chronicles/six-things-developer-tools-must-have-to-earn-trust-and-adoption
- https://www.credo.ai (State of AI Governance 2026: "4% are governing it")
