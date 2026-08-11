# A2 exact-candidate record

Bounded record per `contracts/agent-orchestration.md` §9.

```text
BLOCK: A2
STATE: BLOCKED
BASE: 13cedd6fc1315bfb6fec0c4cacb0eacdb02c6c83 / tree a992bb87382e58d6ec846c7be37cbb941ee0b1b2
CANDIDATE: 80a7d9c328842f1457e866fb8588687e9f1d3118 / tree eaffd3997f140c2c881179e8089ef6bd05b9bc8d
ACCEPTED_TARGET: none
LANDING_EQUIVALENCE: none (accepted identity will equal candidate identity; fast-forward, no rebase)
CHARTER_DIGEST: 522787b1c6f90166b67bdbdef16b47b9cae5819a90a1e6365ceddb87b983632a
CONTEXT_PACKET_DIGEST: see ledger field context_packet_digest
STACK: none
CHANGES: 4 files, tests and documentation only. New test module
         crates/verter_session/src/u6_flow_expect_tests.rs; the two U6 corpus test
         files; a status section in docs/arch/u6-flow-return-gaps-and-target.md.
         No production or semantic source. crates/verter_session/src/lib.rs is
         byte-identical to base.
DELETIONS: six comparison predicates removed as exercised by no row and no control
           (Lit::Bool, Lit::BigInt, alias transparency in two matchers, checker
           Ref<->BareRef, checker Ref<->TypeParam). The governing property is
           DIRECTIONAL MONOTONICITY: each is an accept-arm or transparency removal
           in a matcher total via `_ => false`, so each can only narrow what a pin
           accepts and none can turn a mismatch into a match. The standard a
           deletion must meet: prove the direction of the change over the complete
           match grammar; for a narrowing deletion prove every removed pair reaches
           a controlled fail-closed result; for any deletion that would WIDEN
           acceptance, sample probes are never sufficient — require structural
           unreachability or retain the predicate with a live control; and prove no
           current row depends on the removed vocabulary. Both the architecture and
           adversarial mandates verified this disposition independently.
EVIDENCE: this directory. canonical-verification.md (the canonical pair),
          mutation-campaign.md (comparator discrimination), matrix-coverage.md,
          oracle-profile-stamps.md, red-green-record.md, environment.md,
          command-proofs/ with raw output, digests.txt covering every file and
          verifying with shasum -a 256 -c (exit 0).
REVIEWS: three mandates ran against this exact candidate.
         - architecture (codex gpt-5.6-sol, high): PASS, no blockers. Verified the
           mutation rebind independently: all 79 selectors occur exactly once with
           no planted replacement in the final candidate, the only intervening
           change being comment rewrites.
         - adversarial (Opus): PASS. Enumerated 74 predicates from source
           independently of any manifest; 74 caught, 0 survived, 0 non-runs. Both
           of its earlier P0s closed. Raised two evidence-side P1 blocks, which
           this record and mutation-campaign.md address.
         - conformance (Opus): NOT PROVEN, on an evidence-side P0 only — this
           record previously described a superseded candidate and preserved a
           refuted claim. The code delta was judged conformant. Re-verification of
           the corrected evidence is outstanding.
DISCOVERIES: see below.
NEXT_LEGAL_BLOCKS: A3, once A2 is accepted (program-dag.toml: A3.predecessors = ["A2"]).
MAINTAINER_DECISION_REQUIRED: yes — A2 acceptance, pending conformance
                              re-verification of the corrected evidence.
```

## Discoveries

**D-1 — a deletion on a false premise widened two comparators.** The
`SignatureKind::Call` discriminant was deleted on a sample-probe argument that no
`Construct` signature is reachable on the body-derived rail. Three probes do fail
to reach one: `class C {}` measures `any`, an object type carrying a construct
member measures an empty object, and an alias resolves to a `DeclRef`. A fourth
form reaches it: `function makeProps(x: new () => Box) { return x }` measures a
`SignatureKind::Construct` returning `DeclRef(Box)`. The deletion therefore made
both `ExpectedNode::Signature` and the checker function form accept a construct
signature wherever a call signature was pinned. The discriminant is restored in
both comparators with a bilateral live control on that exact parameter form, and
the control's positive assertion independently proves the node really is measured
as `Construct`. This is why a deletion that could widen acceptance requires
structural unreachability rather than sample probes.

**D-2 — the canonical pair catches what targeted selectors cannot.** Two
candidate-introduced guard failures in this block were invisible to targeted runs
and surfaced only under a full workspace run: `lib_rs_stays_under_line_ceiling`
from module wiring, and `phase_archaeology_test_files_count_zero` from comments
that narrated implementation history instead of stating invariants.

**D-3 — position dependence is two hooks, not one.** The same capture-write cell
refuses in statement, sequence and call-argument positions but publishes a stale
value clean-and-warm in declarator-initializer, if-test, template, short-circuit
and object-literal positions. The refusals come from two independent mechanisms:
the closure-effect refusal reachable only from the expression-statement arm, and
`ValueDescent::UnmodeledCall`. Owned by D5.

**D-4 — N25 is wrong-and-warm.** Measured `{ v: DeclRef(A) | DeclRef(B) | "ok" |
"no" }` against a checker expectation of `{ v: "no" | "ok" }`, with degradation
`None`, admitted warm. Pinned as an expiring `KnownOwed` gap; the block's own
conformance tally moved 7,7,0 to 7,6,1 as a result. Retraction is owned by A3,
the semantic correction by D4, final expiry by D8.

**D-5 — the implementation-side round-4 mutation artifact was not retained.** The
implementation seat's sandbox excluded this evidence directory. Its reported
figures are recorded as unproven; the independent review-side campaign is recorded
there as reported testimony, not as this bundle proof. The campaign this bundle
can prove is the final-blob binding run in `mutation-campaign.md` §3, whose driver
and logs are retained.

## Carried debt

- Consolidate the proof model into one `ObservedFlow`, one `ExpectedFlow` and one
  pure comparator, with stable clause IDs and a manifest that fails when a clause
  lacks a control. Owner D1, before D1 adds further comparator branches.
- Alias transparency is safe by directional monotonicity but has no live control.
- The anti-recurrence floor deliberately does not floor `Primitive`,
  `ObjectSpreadProgram` or `OpaqueOther`; on a shallow row the checker column is
  compared by nothing. Recorded in source. Owner D8.
- `corpus_probe_programs` is a pre-existing early-return no-op inflating the
  suite count by one.
