# AMD-008 round 8 — combined review (post-cut)

Codex xhigh, sandbox read-only, reviewed candidate `b22b8f7356b021311100a4367fcff01c30446ed5`
(the deliberate trim of AMD-008 down to its thesis/criterion/governance core, removing
detailed schema/algorithm/taxonomy prose that had been self-contradicting across rounds
4-7 and reassigning it to the implementation-deliverable vector suite).

## Findings

1. **[conformance]** The control prose ("each mutation must produce the named equality
   RED") did not distinguish comparator mutations from baseline/preflight mutations — a
   code-only baseline mutation should fail the code-baseline assertion, not map equality;
   fail-closed input controls should fail during preflight. Fixed: scoped the equality-RED
   requirement to comparator mutations, added named failure-stage language for baseline
   and preflight controls.

2. **[architecture]** Replacing Required Exits silently dropped the original charter's
   deterministic-serialized-map-output requirement (`BV0A.md:207`) — production's
   `map_hash` is computed over raw serialized bytes (`carrier_compiler.rs:208`), so two
   valid-but-differently-encoded serializations of the same logical artifact could defeat
   it even while passing decoded-artifact equality. Fixed: added back a determinism
   requirement, explicit that it's independent of and additional to the decoded-artifact
   equality check.

3. **[governance]** §4's Abort/rescope replacement was too broad — it dropped BV0A's
   original first-paragraph stop conditions (B3/B4/BV1/B5/universal-IR/new-public-contract).
   Separately, owned-scope item 4's "no harness copy" phrase was left ambiguous against the
   newly-mandated JS harness reference. Fixed: restored the original stop-conditions
   paragraph explicitly unchanged; narrowed "no harness copy" to mean no harness-synthesized
   BF2 candidate / no duplicate production route, explicitly not forbidding the mandated
   test-only reference.

4. **[governance]** §5.1's review table cited the wrong commit for round 7 (`f0a412bd...`,
   which is the commit that added the review *record*, not the one round 7 actually
   reviewed — `623c5e332...`) and overstated that round 7 governance "confirmed" the
   supersession/ratification mechanics, when round 7's own findings 7-8 blocked on exactly
   those grounds. Fixed: corrected the citation; rewrote the claim to accurately attribute
   round 5 architecture's confirmation and state that round 7's governance findings are what
   this round's fix (items 2-3 above) directly resolves.

## Otherwise confirmed correct

The criterion-level "complete artifact" language is meaningful (acceptance still requires
a complete schema covering every field, deferred to the vector suite). The
`UncomposableInputMap` category list reads as non-illustrative — a genuinely exhaustive,
provably-total taxonomy is still required, just not enumerated field-by-field in this text.
No stale lettered-subsection cross-reference remains. §3 and the core §5 freeze/ratification
separation survived the cut intact.

VERDICT: BLOCKING_FINDINGS (4, all narrow and fixed in commit `19cab5d29`)
