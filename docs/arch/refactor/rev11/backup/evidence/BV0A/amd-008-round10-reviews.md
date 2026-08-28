# AMD-008 round 10 — three separate blind mandates

Three independent Codex xhigh dispatches, `--sandbox read-only`, each blind to the
others, all against candidate `7fd910335add65be66fb17d48242b51056a0df48`, tree
`77669faf35a48888dd543542c1f2885f7e6d069e`. Verdicts reproduced verbatim below.

Outcome: conformance `BLOCKING_FINDINGS` (2); architecture `BLOCKING_FINDINGS` (3);
governance `BLOCKING_FINDINGS` (1). NOT ratification-ready. Every finding was
verified against the real cited lines before being recorded; none is fixed on this
tree — the fix-and-review cycle for this round hit its cap and the findings are
handed to the program orchestrator for disposition.

## Round 10 — conformance

1. Blocking — the seed violates the new layer ordering. AMD-008 requires every layer-2 vector to derive from already-frozen layer 1 ([AMD-008:147](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:147>), [AMD-008:171](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:171>), [AMD-008:200](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:200>)). The seed simultaneously says layer 1 does not exist, yet calls itself layer 2 and says its vectors were derived only from V3 and `CodeTransform` ([vectors:2](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:2>), [vectors:5](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:5>), [vectors:14](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:14>)). V4 already decides stable-append/table-remapping and name-propagation semantics ([vectors:257](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:257>)), which are layer-1 chaining/table-policy questions. The seed must either be explicitly non-normative coverage sketches pending post-freeze derivation, or be produced only after layer 1 is frozen.

2. Blocking — §5.1 retains the old single-suite rationale. It says the removed precision prose’s function “is served by the vector artifact and the implementation’s own acceptance review” ([AMD-008:554](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:554>)). The amended document instead assigns that function to independently reviewed, pre-implementation layer 1. This is the old co-developed-suite model in present-tense narration and directly conflicts with §5.1’s own later explanation of the new split at lines 569–583.

The requested source checks otherwise pass: the rename is 7-to-9 bytes and both replacements are global; `Chunk::Overwritten` has the stated non-empty/empty token behavior; and `map_hash` consumes raw map bytes, alongside the source-space token. The production-serialization determinism check is clearly separate from decoded-artifact equality and technically sound. No dangling local links or additional supersession conflicts were found.

VERDICT: BLOCKING_FINDINGS (2)
tokens used
189 761
1. Blocking — the seed violates the new layer ordering. AMD-008 requires every layer-2 vector to derive from already-frozen layer 1 ([AMD-008:147](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:147>), [AMD-008:171](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:171>), [AMD-008:200](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:200>)). The seed simultaneously says layer 1 does not exist, yet calls itself layer 2 and says its vectors were derived only from V3 and `CodeTransform` ([vectors:2](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:2>), [vectors:5](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:5>), [vectors:14](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:14>)). V4 already decides stable-append/table-remapping and name-propagation semantics ([vectors:257](<packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json:257>)), which are layer-1 chaining/table-policy questions. The seed must either be explicitly non-normative coverage sketches pending post-freeze derivation, or be produced only after layer 1 is frozen.

2. Blocking — §5.1 retains the old single-suite rationale. It says the removed precision prose’s function “is served by the vector artifact and the implementation’s own acceptance review” ([AMD-008:554](<docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:554>)). The amended document instead assigns that function to independently reviewed, pre-implementation layer 1. This is the old co-developed-suite model in present-tense narration and directly conflicts with §5.1’s own later explanation of the new split at lines 569–583.

The requested source checks otherwise pass: the rename is 7-to-9 bytes and both replacements are global; `Chunk::Overwritten` has the stated non-empty/empty token behavior; and `map_hash` consumes raw map bytes, alongside the source-space token. The production-serialization determinism check is clearly separate from decoded-artifact equality and technically sound. No dangling local links or additional supersession conflicts were found.

VERDICT: BLOCKING_FINDINGS (2)

## Round 10 — architecture

## Findings

1. **AMD-007 retains a ratified sentence that contradicts the new JavaScript oracle.** The narrow “no harness copy” language is reasonably read as prohibiting a harness-synthesized BF2 candidate map or duplicate production assembler. However, AMD-007 separately states that BV0A must not “let the candidate generate or modify its own oracle” ([AMD-007 §1](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:49)). AMD-008 expressly calls the BV0A-delivered JavaScript reference an “ORACLE” ([AMD-008 §2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:210)). §4 supersedes AMD-007 §1’s correctness statement, but not this separate oracle prohibition ([AMD-008 §4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:411)). Therefore the supersession enumeration is incomplete and leaves stale ratified authority.

2. **Layer 1 does not unambiguously contain all semantic decisions.** The umbrella description says the normative specification includes the canonical output schema and exact sourceless-boundary rules ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:137)), but Layer 1’s enumerated payload substitutes an input DTO, chaining/collision policy, and a write/boundary *manifest* ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:150)). The ratification action repeats that narrower enumeration ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:485)). A manifest identifies boundaries but does not necessarily decide their placement behavior; likewise, canonical field presence, table merge/deduplication/remapping, and optional metadata policy are not expressly frozen. Those decisions can consequently fall into Layer 2’s expected artifacts, recreating the self-consistent common-mode defect—or force another amendment before the gate can operate. Layer 1 must explicitly own every output-field, table, placement, coordinate, and boundary rule.

3. **The initial Layer-1 freeze lacks repository authority and an auditable non-vacuous phase boundary.** AMD-008 says its ratification does not certify Layer-1 content and permits that content to become normative after independent reviews and a recorded digest ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:503)). Revision 11 governance says only the maintainer may accept architecture amendments and that reviewer approvals cannot self-create authority ([governance.md](docs/arch/refactor/rev11/governance.md:12)). Moreover, “written against it” does not require recorded implementation-base identities or prove that prewritten implementations were discarded rather than subsequently validated/rebased. Layer 1 should itself receive a maintainer-ratified pre-implementation lock record bound to commit/tree/blob identities, with the affected implementation surfaces and post-freeze ancestry recorded. Without that, the claimed independent authority and ordering can be satisfied procedurally without reliably closing the common-mode hole.

The remaining checks pass: BV0A’s first abort/rescope paragraph and ownership exclusions remain intact; `CodeTransform` is normative only for the two named sequential rewrites; BV0 retains the literal 36/36 oracle-clean exit; and AMD-008’s citations to AMD-007 lines 501–503 and 528–531 point to the claimed sentences. No tracked files were modified.

VERDICT: BLOCKING_FINDINGS
tokens used
159 856
## Findings

1. **AMD-007 retains a ratified sentence that contradicts the new JavaScript oracle.** The narrow “no harness copy” language is reasonably read as prohibiting a harness-synthesized BF2 candidate map or duplicate production assembler. However, AMD-007 separately states that BV0A must not “let the candidate generate or modify its own oracle” ([AMD-007 §1](docs/arch/refactor/rev11/amendments/AMD-007-assembled-module-source-map-interim.md:49)). AMD-008 expressly calls the BV0A-delivered JavaScript reference an “ORACLE” ([AMD-008 §2](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:210)). §4 supersedes AMD-007 §1’s correctness statement, but not this separate oracle prohibition ([AMD-008 §4](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:411)). Therefore the supersession enumeration is incomplete and leaves stale ratified authority.

2. **Layer 1 does not unambiguously contain all semantic decisions.** The umbrella description says the normative specification includes the canonical output schema and exact sourceless-boundary rules ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:137)), but Layer 1’s enumerated payload substitutes an input DTO, chaining/collision policy, and a write/boundary *manifest* ([AMD-008](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:150)). The ratification action repeats that narrower enumeration ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:485)). A manifest identifies boundaries but does not necessarily decide their placement behavior; likewise, canonical field presence, table merge/deduplication/remapping, and optional metadata policy are not expressly frozen. Those decisions can consequently fall into Layer 2’s expected artifacts, recreating the self-consistent common-mode defect—or force another amendment before the gate can operate. Layer 1 must explicitly own every output-field, table, placement, coordinate, and boundary rule.

3. **The initial Layer-1 freeze lacks repository authority and an auditable non-vacuous phase boundary.** AMD-008 says its ratification does not certify Layer-1 content and permits that content to become normative after independent reviews and a recorded digest ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:503)). Revision 11 governance says only the maintainer may accept architecture amendments and that reviewer approvals cannot self-create authority ([governance.md](docs/arch/refactor/rev11/governance.md:12)). Moreover, “written against it” does not require recorded implementation-base identities or prove that prewritten implementations were discarded rather than subsequently validated/rebased. Layer 1 should itself receive a maintainer-ratified pre-implementation lock record bound to commit/tree/blob identities, with the affected implementation surfaces and post-freeze ancestry recorded. Without that, the claimed independent authority and ordering can be satisfied procedurally without reliably closing the common-mode hole.

The remaining checks pass: BV0A’s first abort/rescope paragraph and ownership exclusions remain intact; `CodeTransform` is normative only for the two named sequential rewrites; BV0 retains the literal 36/36 oracle-clean exit; and AMD-008’s citations to AMD-007 lines 501–503 and 528–531 point to the claimed sentences. No tracked files were modified.

VERDICT: BLOCKING_FINDINGS

## Round 10 — governance / adversarial

1. **BLOCKING — Layer 1 is independently reviewed, but still lacks independent gate authority and a non-retroactive freeze proof.**

   The layer boundary itself is now meaningful: DTO/validation/chaining/collision/boundary semantics belong to layer 1; vectors are coverage only; and a vector cannot settle an omitted semantic question ([AMD-008 §2.1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:150)). That closes the original three-artifact agreement hole only if layer 1 is established independently of the implementation.

   The amendment, however, requires only three reviews and a recorded digest, and expressly says layer-1 semantic content is not ratified here ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:503)). It does not require:

   - an independently appointed layer-1 author/gate authority;
   - designated-maintainer adoption of the exact layer-1 digest;
   - implementation-baseline commits proving neither implementation/prototype existed before the freeze; or
   - prohibition on deriving layer 1 from, or later reusing, the acknowledged superseded implementation.

   An implementer can therefore inspect or build prototypes, author layer 1 to match them, obtain independent reviews, freeze it, and then adapt or recommit those prototypes as implementations “written against” the specification. Because the prototypes predated the specification, the phrase “BEFORE either implementation is written against it” can be satisfied semantically or retroactively without proving that the pass criteria preceded the result direction. A layer-1 digest proves content identity, not this chronology.

   This conflicts with governance’s rule that model approvals cannot create repository authority and that the maintainer alone accepts architecture/gate changes ([governance §1.1](docs/arch/refactor/rev11/governance.md:14)). The BF2 precedent specifically requires gate authority to be separated from implementation and the criteria to be maintainer-ratified before the candidate result is available ([BF2 ruling](docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:38)).

   **Minimum correction:** require the designated maintainer to adopt the exact layer-1 commit/blob as a subordinate amendment or lock record after its three independent reviews and before any BV0A reference or production-composition implementation is authored, reused, translated, or adapted. Record exact JS/Rust preimplementation baseline identities and expressly exclude derivation from or reuse of prior prototypes, including the superseded candidate.

Other requested checks clear:

- **Ratification bundle:** §5’s direct-review alternative or recorded `ONLY`-records diff, byte-identity requirement, and mandatory diff citation prevent silent substantive-byte changes ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:496)).
- **Review history:** rows match the files exactly: rounds 3–7 each have three `BLOCK` verdicts; round 8 has one combined `BLOCKING_FINDINGS (4)` verdict; round 9 has conformance `PASS`, architecture one finding, and governance two findings. The narration accurately attributes the round-7 open authority/boundary issues and round-9 corrections. The corpus contains no round-1/2 verdict artifacts, and §5.1 does not claim they passed or present the current round as settled.
- **AMD-008 ratification authority:** genuinely maintainer-reserved; it remains pending, and review, merge, silence, proposal commit, or preparer action cannot ratify it ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:520)).
- **Supersession:** §4 now reaches all three AMD-007 §8.1 column-delta/chunk-IR directives.

No tracked files were modified.

VERDICT: BLOCKING_FINDINGS (1)
tokens used
221 937
1. **BLOCKING — Layer 1 is independently reviewed, but still lacks independent gate authority and a non-retroactive freeze proof.**

   The layer boundary itself is now meaningful: DTO/validation/chaining/collision/boundary semantics belong to layer 1; vectors are coverage only; and a vector cannot settle an omitted semantic question ([AMD-008 §2.1](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:150)). That closes the original three-artifact agreement hole only if layer 1 is established independently of the implementation.

   The amendment, however, requires only three reviews and a recorded digest, and expressly says layer-1 semantic content is not ratified here ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:503)). It does not require:

   - an independently appointed layer-1 author/gate authority;
   - designated-maintainer adoption of the exact layer-1 digest;
   - implementation-baseline commits proving neither implementation/prototype existed before the freeze; or
   - prohibition on deriving layer 1 from, or later reusing, the acknowledged superseded implementation.

   An implementer can therefore inspect or build prototypes, author layer 1 to match them, obtain independent reviews, freeze it, and then adapt or recommit those prototypes as implementations “written against” the specification. Because the prototypes predated the specification, the phrase “BEFORE either implementation is written against it” can be satisfied semantically or retroactively without proving that the pass criteria preceded the result direction. A layer-1 digest proves content identity, not this chronology.

   This conflicts with governance’s rule that model approvals cannot create repository authority and that the maintainer alone accepts architecture/gate changes ([governance §1.1](docs/arch/refactor/rev11/governance.md:14)). The BF2 precedent specifically requires gate authority to be separated from implementation and the criteria to be maintainer-ratified before the candidate result is available ([BF2 ruling](docs/arch/refactor/rev11/evidence/BF2/debt-BF2-perf-gate-deferred.md:38)).

   **Minimum correction:** require the designated maintainer to adopt the exact layer-1 commit/blob as a subordinate amendment or lock record after its three independent reviews and before any BV0A reference or production-composition implementation is authored, reused, translated, or adapted. Record exact JS/Rust preimplementation baseline identities and expressly exclude derivation from or reuse of prior prototypes, including the superseded candidate.

Other requested checks clear:

- **Ratification bundle:** §5’s direct-review alternative or recorded `ONLY`-records diff, byte-identity requirement, and mandatory diff citation prevent silent substantive-byte changes ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:496)).
- **Review history:** rows match the files exactly: rounds 3–7 each have three `BLOCK` verdicts; round 8 has one combined `BLOCKING_FINDINGS (4)` verdict; round 9 has conformance `PASS`, architecture one finding, and governance two findings. The narration accurately attributes the round-7 open authority/boundary issues and round-9 corrections. The corpus contains no round-1/2 verdict artifacts, and §5.1 does not claim they passed or present the current round as settled.
- **AMD-008 ratification authority:** genuinely maintainer-reserved; it remains pending, and review, merge, silence, proposal commit, or preparer action cannot ratify it ([AMD-008 §5](docs/arch/refactor/rev11/amendments/AMD-008-bv0a-assembly-neutral-exit.md:520)).
- **Supersession:** §4 now reaches all three AMD-007 §8.1 column-delta/chunk-IR directives.

No tracked files were modified.

VERDICT: BLOCKING_FINDINGS (1)

## Verification of the findings (no fix applied on this tree)

Each finding was checked against the real cited lines:

- Conformance 1 — CONFIRMED. The seed artifact now self-identifies as layer 2 while
  stating layer 1 does not exist, and vector V4's own derivation text
  (`assembled-map-composition.vectors.json:256-262`) decides a table
  deduplication/remapping and provenance policy — exactly the "chaining/collision
  policy" the amendment assigns to layer 1. The ordering rule the amendment
  introduces is violated by the artifact it introduces.
- Conformance 2 — CONFIRMED. `AMD-008:559-560` still reads "prose whose function is
  served by the vector artifact and the implementation's own acceptance review",
  the pre-split co-developed model, contradicting §5.1's own later paragraph.
- Architecture 1 — CONFIRMED as a live sentence. `AMD-007:51-52` ("it does not
  reopen BF2 or let the candidate generate or modify its own oracle") is quoted in
  AMD-008 §2 item 4 and argued compatible there, but is NOT enumerated in §4, which
  promises exhaustive sentence-level supersession.
- Architecture 2 and 3, governance 1 — CONFIRMED as design residue on the new
  two-layer mechanism: layer 1's enumerated payload (DTO, validation order,
  chaining/collision policy, write/boundary manifest) is narrower than the umbrella
  sentence it implements (canonical output schema, sourceless-boundary RULES), so
  output-field presence, table merge/dedup/remap, and boundary placement BEHAVIOR
  can still fall to layer 2; and the layer-1 freeze requires only three reviews plus
  a digest, with no maintainer adoption and no chronological proof that no
  implementation or prototype predated it.
