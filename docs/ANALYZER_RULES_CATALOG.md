# Analyzer Rules Catalog — v0.2 (python-fintech-rules/0.2.0)

Deterministic static rules for the Python-first fintech pilot. Every rule is
pattern-level, client-explainable, and carries its known
false-positive/false-negative profile. These rules **do not** find all
financial defects — they find specific, auditable anti-patterns.

Common facts:

- **Language:** Python only. Other languages are backlog, not supported.
- **Evidence model:** every finding carries `file:line_start-line_end`, the
  offending snippet (whitespace-normalized, capped at 160 chars), and the
  BLAKE3 hash of the file as of the workspace snapshot.
- **Provenance:** all rules in this catalog are `detector: static`;
  `model_hint_unverified` is always `false` for them.
- **Deduplication:** identical `(rule_id, location, normalized evidence)`
  tuples collapse into one finding, independent of input order.
- **Remediation readiness policy (all rules):** a static finding without an
  operator-provided reproducible failing test is `candidate_only`. The
  analyzer never auto-claims `remediation_ready`; `manual_review_required`
  is always `true`.

## money-truncation

- **Pattern:** `int(` and `* 100` on the same line — money scaled to cents
  and truncated to integer.
- **Detected risk:** sub-cent fractions silently dropped; systematic money
  loss across many transactions.
- **Severity default:** Critical on money paths
  (billing/fee/payment/charge/price/invoice), otherwise High. Confidence 0.7/0.6.
- **Known FP:** `int(x * 100)` used for non-monetary scaling (percent
  formatting, basis-points display).
- **Known FN:** truncation via `//`, `math.floor`, format specs,
  `Decimal(int(...))`.
- **Proof location:** finding evidence coordinates + snippet.

## money-round-bare

- **Pattern:** a real `round(x)` call (identifier-boundary checked, so
  `settlement_round(` does not match) with no ndigits/policy argument, on a
  money/risk/limit/fee/settlement path.
- **Detected risk:** banker's rounding without a declared policy — charges
  and credits round in ways neither the client nor the customer expects.
- **Severity default:** Critical on money paths, otherwise High.
  Confidence 0.7/0.6.
- **Known FP:** `round()` on non-monetary values inside a finance-named file.
- **Known FN:** rounding hidden in `format()`/f-strings, numpy, or Decimal
  local contexts.

## offbyone-range

- **Pattern:** `range(1, len(...))` — iteration starting one past the first
  element.
- **Detected risk:** a tier, limit or bracket at the boundary is never
  applied (classic fee-tier skip).
- **Severity default:** High on money paths, otherwise Medium.
  Confidence 0.65/0.5.
- **Known FP:** intentional element-0 skips (e.g. diff against previous
  element).
- **Known FN:** off-by-one errors in while-loops, slices, inclusive/exclusive
  bound mismatches.

## none-arith

- **Pattern:** `var = obj.get("key")` without a default, followed by
  arithmetic on `var` within the next two lines.
- **Detected risk:** `None` flowing into arithmetic — runtime crash on a
  fee/settlement path when a profile field is absent.
- **Severity default:** High on money paths, otherwise Medium.
  Confidence 0.6/0.5.
- **Known FP:** `.get()` result validated for None between assignment and use.
- **Known FN:** arithmetic further than 2 lines away, chained optional
  access, missing nested keys.

## todo-marker (finance-path scoped)

- **Pattern:** `TODO` or `FIXME` text in a file whose path matches the
  finance keyword list (billing, fee, payment, charge, price, pricing,
  invoice, money, risk, limit, settlement, tax, discount). Generic-code
  markers are ignored by design.
- **Detected risk:** unfinished logic on a money/risk path is unquantified
  operational risk.
- **Severity default:** Low. Confidence 0.3.
- **Known FP:** informational comments mentioning TODO without unfinished
  work.
- **Known FN:** alternate spellings (To Do, XXX, HACK).

## dangerous-eval (general safety rule, outside the financial set)

- **Pattern:** `eval(` or `exec(` outside comments/docstrings.
- **Detected risk:** arbitrary code execution if the argument is externally
  influenced.
- **Severity default:** Critical. Confidence 0.9.
- **Known FP:** `ast.literal_eval(...)` matches the `eval(` substring but is
  not arbitrary execution.
- **Known FN:** dynamic imports or getattr-dispatched calls.
