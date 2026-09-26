# Pilot Agreement — deterministic_ai_kernel

**PILOT SERVICES AGREEMENT**

This Pilot Services Agreement ("Agreement") is entered into as of
__________ ("Effective Date") by and between:

**Provider:** __________ [legal name], __________ [address] ("Provider")
**Client:** __________ [legal name], __________ [address] ("Client")

Provider and Client are each a "Party" and together the "Parties".

## 1. Purpose and Scope

Provider will deliver a fixed-scope pilot of the deterministic_ai_kernel
execution system (the "System") applied to one mutually agreed code-fix
or migration task class of Client (the "Task Class"), as described in the
Statement of Work attached as Exhibit A ("Pilot").

## 2. Deliverables

Provider will deliver, per Exhibit A:
(a) the System deployed in Client's environment (self-hosted);
(b) an integration of the Task Class into the System, including a
    Layer-1 hint recipe for that class;
(c) a per-run evidence pack for every Pilot execution (plan, prompts,
    responses, seeds, patches, test reports, evidence chain);
(d) a final pilot report with measured success rates against the
    predefined success criteria in Exhibit A.

## 3. Success Criteria

The Pilot's success is measured exclusively against the written criteria
in Exhibit A, agreed before work begins. No other criteria apply.

## 4. Fees and Payment

(a) Pilot fee: **USD 15,000** (fixed), invoiced 50% on signing, 50% on
    delivery of the final report.
(b) **Credit:** if Client executes a license agreement for the System
    within 60 days of the final report, 100% of the Pilot fee is credited
    against the first-year license fee.
(c) Week-0 proof evaluation (Provider's reference defect class or a
    Client-supplied sample) is provided at no charge before signing and is
    not part of the Pilot fee.

## 5. Intellectual Property

(a) Provider retains all rights in the System, including the kernel, the
    hint-engine, and generic hint recipes.
(b) Client retains all rights in Client's code, data, and the evidence
    packs generated from Client's Task Class.
(c) Provider grants Client a perpetual, non-exclusive, royalty-free
    license to use the Client-specific configuration and hint recipes
    developed under the Pilot for Client's internal purposes.
(d) Nothing in this Agreement transfers ownership of the System.

## 6. Confidentiality and Data Handling

(a) Each Party will protect the other's non-public information with
    reasonable care for the term of this Agreement plus three (3) years.
(b) Evidence packs may contain excerpts of Client's code. Provider will
    not remove evidence packs from Client's environment; storage,
    retention, and deletion are Client-controlled. The System runs
    self-hosted; no Client data is transmitted to Provider.
(c) If the Pilot uses a cloud model endpoint, the parties will document
    the endpoint in Exhibit A; prompts sent to that endpoint may contain
    Client code excerpts.

## 7. Honesty and No-Warranty

(a) The System is designed to report failure honestly: it does not and
    will not report a task as successful unless the predefined proof
    (real test execution) passes.
(b) EXCEPT FOR THE FOREGOING, THE SYSTEM AND THE PILOT ARE PROVIDED "AS
    IS". PROVIDER MAKES NO WARRANTY THAT ANY MODEL-GENERATED PATCH IS
    CORRECT; PROVIDER WARRANTS ONLY THAT THE EVIDENCE PACK ACCURATELY
    RECORDS WHAT WAS ASKED, WHAT WAS ANSWERED, AND WHAT RAN.

## 8. Limitation of Liability

Neither Party is liable for indirect, incidental, or consequential
damages. Provider's aggregate liability under this Agreement is capped at
the fees actually paid by Client under this Agreement.

## 9. Term and Termination

This Agreement runs from the Effective Date until delivery of the final
report, unless extended in writing. Either Party may terminate for
convenience on fourteen (14) days' written notice; fees for work already
performed are non-refundable.

## 10. General

Independent contractors. No exclusivity. Assignment only with written
consent. Governing law: __________. This document plus Exhibit A is the
entire agreement.

**Provider:** ____________________  Date: ______
**Client:** ______________________  Date: ______

---

## Exhibit A — Statement of Work

- Task Class: __________
- Client environment (OS, model endpoint): __________
- Duration: four (4) weeks from the Effective Date.
- Predefined success criteria (fill before signing — required):
  1. __________
  2. __________
  3. __________
- Weekly checkpoint call; evidence packs delivered after each run series.
