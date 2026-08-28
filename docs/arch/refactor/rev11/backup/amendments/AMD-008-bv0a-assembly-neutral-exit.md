# AMD-008 — BV0A assembly-neutral exit criterion

**Status:** RATIFIED (see §5.1). Landed at `a1f6523ce`.
**Prepared against:** local `program/architecture-lock` commit
`a5a05c6fa487e2c8937341817bd4cfe6c37019ef`, tree
`e6cf0c7085f3e1efe63868f6439c73d90ddff974`. The BV0A candidate at
`work/bv0a-implementation` (worktree
`<worktree>/verter-bv0a-implementation`) is
SUPERSEDED, not merely non-conforming: §2 deletes the violation-attribution
mechanism that candidate implements, so none of it carries forward. Ratifying
this amendment does not accept any BV0A candidate; a candidate implementing the
amended charter still requires fresh three-mandate review per §5.
**Amends on ratification:** [`../charters/BV0A.md`](../charters/BV0A.md)
(Objective, Required procedure, Required exits, Abort/rescope, owned scope item
2's `CodeTransform`/chunk-IR language, and owned scope item 4's
oracle-validation clause), the mirrored passages plus the correctness
definition in [AMD-007](AMD-007-assembled-module-source-map-interim.md)
(including two column-delta directives in AMD-007's own recorded
ratification), and [`../charters/BV0.md`](../charters/BV0.md) (Required exits,
reinforced only) — full enumeration in §4. **Introduces one new normative
specification in two layers** (§2 item 1, §5): a frozen semantic
specification, independently reviewed and frozen BEFORE either implementation
is written against it, and the literal vector coverage set at
`packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json`,
committed as a reviewed seed under this amendment and completed and frozen as
a BV0A acceptance deliverable. Does not touch the DAG, BF2, BF3, B2/B3/B4,
BV1, B5, or EM-038.

## 1. Binding direction

BV0A's implementation attempt proves a real, valuable, but NARROWER claim than
BV0A's ratified Required Exits: assembly composition is provably innocent of
*some* class of new violations. Three independent reviews and a subsequent
architecture ruling found the narrower claim, and the mechanism first drafted to
express it, insufficient — all confirmed by direct source reading, not taken on
the implementer's word:

1. **Violation attribution is the wrong instrument.** Judging composition by
   matching the oracle's VIOLATIONS between an assembled run and per-fragment
   standalone runs requires an identity for each violation, and the oracle's
   violation type is only `{ rule, detail }` — so an identity can be recovered
   only by parsing eighteen families of human-readable diagnostic prose and
   rewriting the oracle's human-readable range labels. That makes a private
   formatting grammar into an architectural contract, and it still cannot see
   defects that produce no violation at all. `db26cde00`'s `probe_fragments`
   also ran each fragment against the FIXTURE's complete anchor set, so a
   script probe reported missing-template-anchor violations and vice versa, and
   its pooled `violation_key` match could consume an assembly-introduced
   violation from the other fragment's spurious entry — confirmed against the
   real committed test code (`crates/verter_vue_conformance/tests/common/bf2.rs`,
   `crates/verter_vue_conformance/tests/cases/official_seed_matrix.rs`).

2. **A multiset of segments is not the composition invariant; the ordered
   sequence is.** V3 permits several segments at one generated coordinate, the
   accepted decoder preserves their wire order, and the oracle's own lookup
   takes the LAST applicable segment at or before a column — so reordering two
   equal-coordinate segments changes which authored position a consumer
   resolves while leaving any multiset identical. BV0A's owned scope already
   requires preserving "segment order", so a multiset comparison drops a
   requirement the charter states outright.

3. **A bespoke offset formula is the wrong model for the two rewrites.** The
   rewrites are code-and-map transforms and already have normative semantics in
   `CodeTransform`: a non-empty overwrite emits one token at the replacement
   start mapped to the overwritten range's original start, rather than
   preserving and clamping every segment inside the replaced range. A separate
   point-rebasing/clamping policy invented alongside it is a second model of the
   same operation, and the two disagree.

The root technical cause of the underlying emitter defects (traced by an
independent architecture consult,
[`../evidence/BV0A/circularity-consult.md`](../evidence/BV0A/circularity-consult.md),
confirmed accurate by independent conformance re-reading of the cited source):
`crates/verter_compiler/src/script/process.rs`'s `overwrite_or_root_prefix`
replaces `<script setup>`'s opening-tag range with a generated wrapper
beginning `const __sfc__...`; `Chunk::Overwritten`
(`crates/verter_compiler/src/code_transform/source_map.rs:243`) emits one
source-bearing segment at the replacement's generated start pointing to the
overwritten range's ORIGINAL start — the authored `<` of `<script...`. A
synthetic-boundary classification bug, not a global offset error; fixable at
the emission operation. Missing required-anchor coverage across
script/VDOM/Vapor/SSR paths is a separate, broader emitter-fidelity gap. Both
are squarely **BV0's** owned scope per AMD-007 §4 ("BV0 retains any residual
script-emitter, template-emitter, or Vue-semantic mapping correction exposed by
the accepted map"), not BV0A's — but BV0 is `LOCKED` until BV0A is accepted,
because BV0A precedes BV0 in the DAG (`BF2 -> BV0A -> BV0`). This is a genuine
circularity in AMD-007's ratified text, undiscovered during its six review
rounds because those rounds validated the charter's logical soundness before
an implementation existed to run the real oracle against the real (defective)
emitters.

**Resolution:** BV0A's acceptance boundary is redefined per §2 below — exact
ORDERED equality of the complete decoded map artifact against an independently
computed, input-only reference, under real `CodeTransform` rewrite semantics,
with oracle-violation attribution deleted entirely. This is both simpler and
materially stronger than attribution: it catches raw corruption that stays
oracle-valid, corruption that collides with an inherited violation, and
corruption that produces no violation at all. BF2 is NOT reopened; its oracle
and invocation are untouched. BV0's own literal 36/36 fully-oracle-clean exit
is unchanged and explicitly reinforced (§3): BV0A's narrower exit does NOT
relieve BV0 of ultimate responsibility for the full clean verdict once both
blocks are accepted.

## 2. BV0A charter amendment

On ratification, [`BV0A.md`](../charters/BV0A.md) is amended in five bounded
ways. **This section states the thesis, the acceptance criterion, and the
governance rule that a separately reviewed and frozen semantic specification
plus its literal vectors — not prose here — are the normative specification of
the composition algebra. It deliberately does not attempt to re-derive that
algebra in prose.** Five review rounds established that prose
cannot settle this class of precision: each round closed the named edge cases
and the next round found the next layer, including two rounds where the
amendment's OWN attempted precision text contradicted itself. Literal
input/output vectors are the right tool for this problem, the same way the
source-map, Unicode, and WebAssembly specifications settle it.

1. **Objective** — "Correct" is redefined to: "the assembled module's code is
   byte-identical to the pinned pre-amendment baseline, its emitted map is a
   valid flat source-map v3 artifact, and that artifact equals an
   independently computed reference artifact EXACTLY, field for field and
   position for position, including the exact ORDERED sequence of segments (a
   multiset or sorted comparison is forbidden — BV0A's owned scope already
   requires preserving segment order, and reordering two equal-coordinate
   segments changes which authored position a real consumer resolves even
   though it leaves any multiset identical)."

   The compared artifact is the complete decoded map object — never an oracle
   classification, a relation, or a violation report. Wire validity (a
   spec-conformant decode, not merely "did not throw") is checked before any
   logical comparison. Authored `original` coordinates, source spellings, and
   names are carried OPAQUELY and unchanged; whether they tell the truth about
   the authored SFC is BV0's concern, not BV0A's — BV0A's objective is only
   that composition neither invents, drops, reorders, duplicates, nor perturbs
   them.

   **The normative specification of the composition algebra — the exact
   canonical output schema, the exact chaining/transform algebra for both
   authorized rewrites, and the exact rules for assembly-owned sourceless
   boundaries — is a separate reviewed artifact plus its literal vectors at**
   `packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json`,
   not prose in this document. Ratifying this amendment ratifies that
   MECHANISM — that a frozen specification and its vectors, not prose here,
   govern — and does not itself certify any of that specification's semantic
   content, nor the seed vector artifact as complete or correct.

   **The specification has TWO layers, and they are deliberately NOT developed
   in the same window.**

   - **Layer 1 — the frozen semantic specification.** The pre-assembly input
     DTO schema; the validation order and the exhaustive
     `UncomposableInputMap` rejection taxonomy; the chaining/transform algebra
     for both authorized rewrites, including equal-coordinate ordering and
     collision policy; and an exhaustive manifest of the real assembler's
     write sites and of every transition between mapped fragment content and
     assembly-owned synthetic content. Layer 1 is its OWN artifact. It
     receives its own independent conformance, architecture, and adversarial
     review, and is FROZEN at an exact commit and blob digest BEFORE the
     independent reference of item 2 and the production Rust assembler are
     written against it. No equality comparison between those two
     implementations is acceptance evidence until layer 1 is frozen. After
     that freeze, changing layer-1 semantics requires its own amendment — it
     is not a BV0A implementation decision, and it is not something a vector
     can do.
   - **Layer 2 — the coverage set.** The enumerated literal vectors that
     exercise layer 1's semantics, in the artifact named above. Layer 2
     remains a **BV0A implementation deliverable**, built out incrementally
     and independently reviewed as it completes, and a precondition of BV0A's
     ACCEPTANCE rather than of this amendment's ratification. It is FROZEN at
     BV0A's acceptance; changing, adding, or removing a vector thereafter
     requires its own amendment, exactly as changing this text does. Every
     vector must conform to already-frozen layer-1 semantics. A vector that
     would settle a question layer 1 did not already decide is not a coverage
     addition — it is a semantic change, and it requires an amendment.

   **Why the split, stated plainly.** If the vectors, the independent
   reference, and the production implementation are all produced in one
   window, the same wrong rule — a collision ordering, a boundary transition,
   a rejection category — can be encoded in all three, self-consistently.
   Every vector then reproduces, exact equality holds, every mutation passes,
   and the vector-precedence clause turns the shared error into the contract.
   Exact equality between two implementations proves AGREEMENT, not
   correctness; it is a sufficient oracle only against semantics fixed by an
   authority neither implementation controls. This is the program's existing
   rule that a candidate cannot choose its own pass criteria
   (`governance.md`, Gate authority — applied to a performance gate in
   [`../evidence/BF2/debt-BF2-perf-gate-deferred.md`](../evidence/BF2/debt-BF2-perf-gate-deferred.md)),
   applied here to SEMANTIC criteria, where the same failure mode is available
   and harder to see.

   This two-layer split is also where the defects found in rounds 4 through 7
   (a schema/DTO underspecification, two wrong vector-derivation examples, a
   source-table contradiction, an incomplete failure taxonomy, insufficient
   boundary-geometry coverage) land. Every one of them is layer-1 content.
   Moving them out of this document's ratified prose is therefore not a
   deferral into implementation judgment: they are deferred into an artifact
   that is independently reviewed and frozen before either implementation is
   written against it.

   Each layer-2 vector must carry its own derivation — the layer-1 rule it
   pins and why its expected output follows from the V3 wire format and
   `CodeTransform`'s real chunk-emission semantics — and must be hand-derived
   from layer 1 and the real chunk semantics, never generated by either
   implementation; both the independent reference and the production Rust
   assembler must reproduce every vector exactly. Where prose elsewhere in
   this charter and a frozen vector could be read to disagree, the vector
   governs. Where a vector and frozen layer 1 could be read to disagree,
   LAYER 1 governs and the vector is the thing that is wrong.

2. **Required procedure** — REPLACED IN FULL. The former three-probe, per-rule
   violation-identity, and injective-matching procedure is deleted, together
   with the standalone fragment probes, the generated-only-range label
   rewriting, and every rule that derived an identity by parsing the oracle's
   human-readable `detail` text. Attribution of oracle violations is not part
   of BV0A's acceptance at all. In its place, BV0A lands a test-only,
   independent, cross-language reference that computes the expected map
   artifact from inputs alone, meeting three cumulative, structurally
   auditable requirements:

   - **Location and language.** The reference lives in the JavaScript
     conformance harness (`packages/framework-conformance-harness/`) with NO
     dependency — import, FFI, generated binding, or fixture produced by — on
     Rust composition, rewrite, placement, or map-emission code. A second Rust
     implementation beside the first does not satisfy this.
   - **Input-only interface.** The reference consumes one serialized
     pre-assembly input DTO carrying every input the real assembler reads (the
     exact field list is layer-1 content per item 1 — fixed and frozen before
     this reference is written, not ratified here and not settled by the
     reference) and nothing else — never the production map, splice lists,
     placement traces, or composition helpers.
   - **No translation.** The reference is written from frozen layer 1, this
     amendment's thesis, and the real `CodeTransform`/V3 semantics, not
     transcribed from the production implementation or its diff — the property
     that closes common-mode error between the two implementations, which is
     also why layer 2 requires literal hand-authored vectors produced by
     NEITHER implementation, and why layer 1 is frozen before either exists.

   The reference is an ORACLE, not a second production assembler: it is never
   supplied to BF2 as a candidate map. AMD-007's prohibition on a
   harness-synthesized candidate map stands unchanged — BF2 receives only the
   genuine production map; this mandated test oracle is not the thing that
   prohibition forbids.

   The two authorized rewrites are modeled as real `CodeTransform` code-and-map
   transforms applied SEQUENTIALLY (pass one globally overwrites every
   `__sfc__` with `_sfc_main`; pass two, on pass one's output coordinate
   space, globally removes every `export default _sfc_main;\n`) — not as a
   bespoke offset/clamp formula over decoded positions. Token geometry follows
   `Chunk::Overwritten`: a non-empty overwrite emits one token at the
   replacement's generated start, mapped to the overwritten range's original
   start; an empty overwrite emits no replacement token. This mandates
   `CodeTransform`'s LOCAL rewrite semantics for these two rewrites only — it
   does not mandate a whole-module or cross-block chunk IR, and confers no B4
   authority. Provenance (script/template/assembly-boundary origin) is tagged
   at ingestion and survives rewriting, placement, and table remapping as
   composition-time bookkeeping; it is never inferred from final coordinates
   or spelling, and the emitted wire map does not serialize it.

   Acceptance evidence for a BV0A candidate requires: an independently
   reviewed layer-1 specification, frozen at a recorded commit and blob digest
   BEFORE either implementation was written against it, with that digest
   cited; full conformance to the complete frozen layer-2 vector suite (every
   vector executed and reproduced exactly, with the executed count asserted
   against the suite's own inventory); a comprehensive positive fixture exercising real assembler
   geometry (both rewrites, mid-line and terminal removal, equal-coordinate
   ordering, sourceless boundaries, astral/CRLF text, duplicate table
   spellings); mutations proving the ordered-equality comparator actually
   discriminates order, rewrite geometry, chain bias, placement, synthetic
   provenance, and every compared artifact field individually (not just
   geometry — a comparator that silently ignores one field must be provably
   caught); a fail-closed control per `UncomposableInputMap` category (each
   failing at its own preflight validation stage, before any artifact
   comparison runs); and a pinned-baseline control proving a production-only
   code mutation produces a named RED at the CODE-baseline assertion
   specifically (not the map-equality assertion — a code-only mutation should
   leave the independent reference's map expectation unaffected). Each
   mutation must prove the plant was present, unique, and new; the reference
   was unchanged where the mutation's own category does not target it; the
   correct NAMED assertion for that mutation's category (comparator,
   preflight, or baseline — never an unrelated build/setup failure) produced
   the RED; and the original identity was restored and reverified GREEN. The
   per-category enumeration these controls run against is layer-1 content; the
   controls themselves are layer-2/implementation work reviewed alongside the
   suite — neither is re-litigated here.

3. **Required exits** — REPLACED. The former full-oracle-pass exits ("every
   source-bearing segment ... satisfies one of the oracle's declared
   relations", "no mapping check is skipped ... every planted mutation is
   detected" as an unconditional whole-oracle requirement) are removed, as is
   the interim "zero unattributed violation" formulation. In their place:

   > Cell applicability is partitioned from the LOCKED BF2 seed manifest's own
   > `sourceMap` request input — never from candidate map presence or any
   > production-produced metadata — and every one of the 36 cells is
   > accounted for as map-enabled or map-disabled with none unclassified.
   >
   > For every map-enabled cell, the genuine production assembler returns
   > code and a map together; the code is byte-identical to the pinned
   > pre-amendment baseline; the emitted map passes independent wire
   > validation; the PRODUCTION serialization is deterministic across
   > repeated identical invocations (this is independent of, and in addition
   > to, artifact-level equality — production's own `map_hash` is computed
   > over raw serialized bytes, so two valid but differently-encoded
   > serializations of the same logical artifact would defeat that hash's
   > purpose even though they would pass the decoded-artifact comparison
   > below); and the complete DECODED artifact equals the independently
   > implemented, input-only reference exactly — under the two authorized
   > sequential `CodeTransform` rewrites and frozen layer 1's chaining algebra
   > — with item 1's layer-1 semantic specification independently reviewed and
   > frozen at a cited digest BEFORE either implementation was written against
   > it, and its layer-2 vector suite delivered, independently reviewed, and
   > reproduced exactly by both implementations, before this exit is claimed.
   > For every map-disabled
   > cell, no map is produced, and that absence is asserted independently
   > rather than by omitting the check.
   >
   > Missing or uncomposable required input maps fail closed (item 4). BF2's
   > authored-source oracle runs once per cell over the genuine production
   > result through its accepted entry point, unchanged, and the candidate
   > records that it RAN for every cell; only its non-clean MAPPING verdict is
   > excluded from BV0A's gate. Residual fragment-emitter violations are
   > BV0's acceptance responsibility, not BV0A's.

   Unchanged: the typed code-plus-map production result with no
   harness-synthesized BF2 candidate map and no duplicate production assembly
   path; no fabricated provenance over assembly scaffolding; all pre-existing
   assembled-code bytes and other axes (parse, link, structure, runtime/server,
   diagnostic, route) unchanged; applicable locked performance cells within
   existing thresholds; no B3/B4/BV1/B5 authority, no universal IR, no waiver
   artifact.

4. **Abort/rescope** — BV0A's first Abort/rescope paragraph (the `RESCOPE_REQUIRED`
   stop conditions for B3's canonical request, B4's general architecture,
   BV1's complete Vue plan, B5's direct-core cutover, a universal or
   cross-framework IR, a new public product contract, and BF2 oracle/invocation
   immutability) is UNCHANGED and remains in full force — this amendment does
   not touch it. Only the SECOND paragraph's "absent, false, or uncomposable"
   trigger is replaced, because it was undefined enough to be read either way
   after a failure was already visible: `UncomposableInputMap` — a structural
   input defect BV0A cannot faithfully carry forward, causing a hard
   fail-closed (or rescope to the true owner), never coerced into an empty,
   approximate, or unmapped successful result — is a genuinely exhaustive,
   spec-derived taxonomy (malformed map JSON; wrong/missing version;
   undecodable or out-of-range wire data; malformed table rows; an
   indexed/non-flat map; a dangling table index; an out-of-fragment or
   surrogate-split coordinate; incompatible cross-fragment table metadata) —
   its exact, provably-total enumeration and its validation order are layer-1
   content per item 1, fixed and frozen before either implementation is
   written against them; the per-category fail-closed controls are
   implementation deliverables. Neither is re-litigated field-by-field here.

   Owned-scope item 4's "no harness copy" phrase (unchanged text, carried from
   AMD-007) means specifically: no harness-synthesized BF2 CANDIDATE map, and
   no duplicate PRODUCTION assembly route. It does not forbid, and was never
   read by AMD-007 to forbid, the independent JavaScript reference item 2
   mandates — that reference is a test-only oracle, never supplied to BF2 as a
   candidate and never a second production path. This reading is stated
   explicitly here because item 2 makes concrete what AMD-007 only mandated in
   the abstract ("BV0A supplies the missing candidate artifact ... it does not
   reopen BF2 or let the candidate generate or modify its own oracle").

   Two exclusions are explicit, because both are otherwise available as an
   escape:

   - A template-only cell whose compiler produced a SYNTHETIC script block
     with an empty map is NOT a missing required map — it is synthetic
     sourceless code, composed as such. Map-requiredness comes from the
     pre-assembly authored-fragment inventory, never from
     `compiled.script.is_some()`.
   - A mechanically composable but oracle-INVALID fragment mapping — one that
     decodes and composes cleanly while pointing somewhere the authored
     fixture does not justify, such as the diagnosed `const`-to-`<` segment —
     is NOT grounds for `RESCOPE_REQUIRED` and NOT a BV0A defect. It is
     carried forward faithfully and is a mandatory BV0 bug.

   An exact-equality failure against the reference is BV0A's OWN composition
   defect by definition, and is fixed in BV0A rather than rescoped.

5. **Owned scope items 2 and 4** — item 2's sentence making `CodeTransform` "an
   optional fallback only for a genuinely more complex rewrite", and the
   Required-procedure paragraph stating that "these two structurally known
   cases do not require `CodeTransform` or a general edit/chunk abstraction",
   are replaced: `CodeTransform`'s local code-and-map semantics are NORMATIVE
   for both rewrites per item 2 above. The custom per-occurrence column-delta
   and line-and-column splice geometry described there is deleted, not
   repaired. Item 4's "validated through BF2's reaccepted authored-source
   oracle" is narrowed to mean DELIVERY to the unchanged oracle and a recorded
   run, not a clean verdict. Everything else in both items is untouched — in
   particular both rejected identifier renames (`__sfc__` and `_sfc_main`)
   remain rejected for exactly the recorded cross-consumer reasons, and the
   removal remains GLOBAL rather than suffix-only.

## 3. BV0 charter reinforcement

[`BV0.md`](../charters/BV0.md)'s existing Required Exits language ("mapping
check ... genuinely invokes BF2's accepted authored-source mapping oracle")
already requires the full 36-cell oracle-clean result. This amendment adds one
clarifying sentence, non-substantive: "This exit is not satisfied by BV0A's
narrower composition-equality proof (AMD-008), which deliberately does not gate
on the oracle's verdict — BV0 owns bringing every residual violation BV0A's
composition faithfully carried forward to zero, across script, VDOM, Vapor, and
SSR paths, before this exit is met." No other BV0 charter text changes.

## 4. Scope of amendment and supersession

This amendment supersedes exactly the following and no more. The list is
enumerated sentence by sentence rather than summarized, because an
acceptance-boundary change that leaves a conflicting ratified sentence standing
has not actually changed the boundary.

1. **The acceptance-boundary sections.** BV0A's Objective, Required procedure,
   Required exits, and Abort/rescope, in `BV0A.md` and in their mirrored
   AMD-007 §3 text alike — replaced per §2 items 1–4.
2. **The correctness definition.** AMD-007 §1's statement that BV0A owns "a
   real, correct map — correct under BF2's accepted authored-source contract",
   and AMD-007 §7's statement that mapping correctness is governed by BF2's
   oracle. Correctness is redefined per §2 item 1; BF2's oracle remains
   connected and unchanged but is not the correctness gate.
3. **The `CodeTransform`/chunk-IR language, in all six mirrored charter
   occurrences (three in `BV0A.md`, three in AMD-007) plus the three
   ratification-record sentences in item 4 below.**
   Owned scope item 2's "optional fallback only" sentence and the
   Required-procedure paragraph beginning "These two structurally known cases
   do not require `CodeTransform`", in BOTH `BV0A.md` and AMD-007; and the
   retained post-list paragraph in both documents stating the interim design
   "does not require a chunk IR" — replaced per §2 item 5, which mandates
   `CodeTransform`'s LOCAL rewrite semantics only and still forbids a
   whole-module or cross-block chunk IR.
4. **The three column-delta / chunk-IR sentences inside AMD-007's recorded
   ratification (§8.1).** All three are superseded to exactly the extent §2
   item 5 replaces the custom per-occurrence column-delta model with
   `CodeTransform`'s LOCAL rewrite semantics for the two authorized rewrites,
   and no further:
   - the ratification blockquote's "no CodeTransform/chunk-IR mandate" clause;
   - the narrative sentence recording that "The maintainer confirmed keeping
     the already twice-independently-verified per-occurrence column-delta
     approach and proceeding to ratification without either rename"
     (AMD-007:501-503) — superseded only as to the column-delta approach; the
     rename rejection it also records stands, and is reaffirmed in §2 item 5;
   - the closing **Maintainer decision: RATIFIED** sentence's "with direction
     to keep the column-delta approach and proceed" (AMD-007:528-531) —
     superseded as to that direction only; the recorded RATIFIED decision
     itself, and its approval of everything else in that blockquote, stands.

   These last two sit outside the six mirrored charter occurrences of item 3,
   so item 3 does not reach them; left unlisted they would remain live
   authority directly contradicting §2 item 5. Superseding text inside a
   recorded maintainer decision is unusual and is therefore enumerated
   explicitly rather than left to inference; the rest of that decision —
   including the DAG edit, the acceptance ordering, the identifier-rename
   rejections, and BV0's retained literal 36/36 exit — stands untouched.
5. **BV0A owned scope item 4.** "validated through BF2's reaccepted
   authored-source oracle" is narrowed per §2 item 5 to mean delivery to the
   unchanged oracle and a recorded run, not a clean verdict.

It adds the one clarifying sentence to `BV0.md` in §3 above.

It does NOT supersede AMD-007's DAG edit, the remainder of BV0A's owned-scope
list (still forbidding B3/B4/BV1/B5 authority, both rejected identifier
renames, and any oracle or invocation change), §5's EM-038 preservation, or any
other AMD-005/006/007 content. In particular, AMD-007 §1's sentence that BV0A
"does not reopen BF2 or let the candidate generate or modify its own oracle"
(AMD-007:51-52) is NOT superseded and remains live, compatible authority: the
§2 item 2 JavaScript reference is a test-only ORACLE that checks the candidate,
never a thing the candidate generates or modifies, and it is never supplied to
BF2 — it does not touch, reopen, or become BF2's oracle, so no conflict exists
and nothing here needs to supersede it. This is stated explicitly, not left to
the discussion in §2 item 2, because this section enumerates supersession
sentence by sentence and an unstated compatibility claim reads the same as an
overlooked one.

It does NOT reopen BF2. The oracle's rules, relation table, anchors, violation
type, and invocation are untouched; §2 keeps BF2 running once per cell over the
genuine production result through its accepted entry point. A future
structured-diagnostic API for the oracle would be an independent BF2
maintainability change, and is neither required by nor authorized by this
amendment.

## 5. Exact ratification action

After independent conformance, architecture, and adversarial/governance review
close `PASS` on one exact reviewed amendment-package commit and tree binding
this amendment's charter deltas and the seed vector artifact, the designated
maintainer records:

> Ratify AMD-008 for reviewed package commit `<reviewed-full-sha>`, tree
> `<reviewed-tree-oid>`, and ratification-bundle commit `<bundle-full-sha>`,
> tree `<bundle-tree-oid>`; confirm BF2's accepted oracle and invocation are
> unchanged and unreopened, and that BV0A's owned-scope boundary (no identifier
> rename, no B3/B4/BV1/B5 authority) is otherwise unchanged; redefine BV0A's
> acceptance boundary to the exact §2 ordered map-artifact equality
> specification against an independent input-only reference; make
> `CodeTransform` semantics normative for both authorized rewrites per §2 item
> 5; establish §2 item 1's two-layer artifact as the normative specification
> of the composition algebra — layer 1 (DTO schema, validation order and
> rejection taxonomy, chaining/collision policy, exhaustive assembler
> write/boundary manifest) independently reviewed and FROZEN at a recorded
> commit and blob digest BEFORE either implementation is written against it
> and changeable only by a further amendment thereafter, and layer 2 (the
> literal vector coverage set) a reviewed BV0A ACCEPTANCE deliverable rather
> than a condition of this ratification, frozen at that acceptance and
> changeable only by a further amendment thereafter; reinforce BV0's literal
> full-oracle-clean exit per §3.

**The ratification bundle may not carry unreviewed bytes.** Either all three
reviews bind the final ratification-bundle commit and tree directly, or the
bundle's diff against the reviewed package must be recorded and must contain
ONLY the review-history and §5.1 ratification records, leaving this amendment's
text and its charter deltas byte-identical. The recorded decision states which
of the two applies and cites the diff.

This ratification action approves the AMENDMENT TEXT and the seed vector
artifact only. It does not certify any layer-1 semantic content: the layer-1
specification of §2 item 1 is NOT ratified here, and must itself close
independent conformance, architecture, and adversarial review and be frozen at
a recorded digest before either the independent reference or the production
assembler is written against it. It does not accept any BV0A candidate either.
A candidate implementing this amended charter — including a frozen,
independently reviewed layer-1 specification and a completed, frozen layer-2
vector suite with its own independent review — must separately receive fresh
conformance, architecture, and adversarial review, all closing `PASS` on one
exact candidate commit and tree, before BV0A is accepted. The candidate at
`work/bv0a-implementation` is superseded rather than corrected: its standalone
probes, `violation_key`/`violation_multiset` matching, and custom splice
geometry are deleted by §2, so no part of its attribution mechanism carries
forward. The prior conformance `FAIL` on it is not reinterpreted, waived, or
grandfathered by this amendment's own ratification.

Silence, review, merge, or this proposal's commit is not ratification. Any
changed reviewed-package byte requires regenerated identities and fresh
reports. The preparer cannot ratify, review, or satisfy any independent
mandate.

### 5.1 Recorded ratification

**RATIFIED.**

Review history, all verdicts recorded verbatim in
[`../evidence/BV0A/`](../evidence/BV0A/):

| Round | Reviewed commit | Design under review | Verdict |
| --- | --- | --- | --- |
| 3 | `e78512f23108d532be607361e774eda52b193001` | injective per-rule match over oracle violations | 3x `BLOCK` |
| 4 | `9427e0378b91254d1f05bdbee1c8a6c9b61f9981` | ordered map-artifact equality, prose-only algebra | 3x `BLOCK` |
| 5 | `2096ae221c3f8860299ac89ec67d31f9cec36149` | same, prose algebra completed | 3x `BLOCK` |
| 6 | `bf08e8ae8e4a28802b73a5d5e719adf3f170c31e` | algebra moved to frozen literal vectors | 3x `BLOCK` |
| 7 | `623c5e33265bc57ff93c1791ce52d09dd5ea57c7` | vectors scoped to acceptance; `file`-absence decided | 3x `BLOCK` |
| 8 | `b22b8f7356b021311100a4367fcff01c30446ed5` | surplus precision prose cut to the validated core | 1x `BLOCKING_FINDINGS` (4) — one COMBINED-mandate review, not three separate votes (narrow: mutation failure-stage wording, a dropped determinism exit, an over-broad abort/rescope replacement, a stale round-7 citation) |
| 9 | `2f484af88b5c0f85f99d7f142084e2b7e102ec99` | same core, three separate blind mandates | conformance `PASS`; architecture `BLOCKING_FINDINGS` (1); governance `BLOCKING_FINDINGS` (2) |
| 10 | `7fd910335add65be66fb17d48242b51056a0df48` | two-layer specification split (frozen layer-1 semantics, layer-2 coverage) | conformance `BLOCKING_FINDINGS` (2); architecture `BLOCKING_FINDINGS` (3); governance `BLOCKING_FINDINGS` (1) |
| 11 | `3a120f81dfb144e6d02e5374822eec6c6d764104` | round-10 mechanical fixes plus the `FC-VUE-003` debt disposition | combined check `PASS`; final independent 3-way: conformance `PASS`, architecture `PASS`, governance `BLOCKING_FINDINGS` (2) — the debt row's chronology check was assess-only rather than disqualifying, and §5.1 overclaimed layer-1 "adoption"; both fixed |

Round 3 ended the violation-attribution design; an independent architecture
ruling directed replacing it, and §2 is that replacement. Rounds 4 through 7 did
NOT challenge the replacement's thesis — ordered-artifact equality, the
`CodeTransform` rewrite model, deleting attribution, and not reopening BF2 all
survived every round, and reviewers affirmatively confirmed the rewrite order,
the 7-to-9 byte rename, the `Chunk::Overwritten` token geometry, the ownership
model, and BV0's retained exit. What rounds 4 through 7 established instead is
that PROSE cannot settle the composition algebra at implementation-grade
precision: each round closed the named edge cases and the next found the next
layer, including contradictions introduced by the fix passes themselves
(a stale dual freeze-point statement, a self-contradicting source-table rule).

**Round 8's revision removed that surplus prose rather than patching it
again**, per round 7's own architecture finding ("some explanatory chaining
prose should be cut once vectors govern; retaining both invites conflicting
readings") and the program orchestrator's review of the accumulated pattern —
four consecutive rounds where fix passes closed roughly as many defects as
they introduced, all inside prose whose function is now served by the layer-1
specification (independently reviewed and frozen before either implementation
is written against it, per §2 item 1 — adoption as a maintainer lock record is
prospective future work gated on closing `FC-VUE-003`, not something this
ratification performs) and the layer-2 coverage set's own acceptance
review — not, as an earlier draft of this narration said, by "the vector
artifact and the implementation's own acceptance review" undifferentiated;
that undifferentiated framing was the pre-split co-developed model round 9
found insufficient, and restating it here would contradict this section's own
later explanation. Round 8 was ONE
combined-mandate review returning a single `BLOCKING_FINDINGS` verdict, not
three separate votes; round 9 restored three separate blind mandates. Round 5
architecture independently confirmed the circularity/supersession scope
correct; round 7 governance did NOT confirm the supersession/ratification
mechanics — its own findings blocked on bundle-hygiene and
supersession-completeness gaps, which round 8 fixed directly rather than
claiming a prior confirmation that did not happen.

**What round 9 found, and what this round's revision changes.** Moving the
detailed schema, chaining derivation, and control enumeration out of ratified
text — the round-6/7 vectors-scoping decision — settled WHERE that content
lives. It did NOT settle WHO has authority over it, and this document
previously read as though it had. Round 7 architecture had left exactly that
open (its findings 4 and 5: co-developed specification authority permits
common-mode defects; the boundary proof is not closed without an exhaustive
assembler write/boundary manifest), and round 9 governance found it still
open — a wrong collision or boundary rule could be encoded identically in the
vectors, the independent reference, and production, pass every check
self-consistently, and be settled in its own favour by the vector-precedence
clause. §2 item 1 now splits the artifact into a layer-1 semantic
specification, independently reviewed and frozen at a digest BEFORE either
implementation is written against it and changeable only by amendment, and a
layer-2 coverage set that remains a BV0A acceptance deliverable. Round 9
architecture separately found §4's supersession enumeration incomplete — two
column-delta directives inside AMD-007 §8.1 lay outside the six mirrored
charter occurrences and remained live conflicting authority; §4 item 4 now
enumerates all three §8.1 sentences. Round 9 conformance closed `PASS`.

**What round 10 found, and this revision's final disposition.** Round 10
confirmed the layer split itself is sound and that supersession now reaches
all three §8.1 directives, and found four further findings, all verified
against real cited lines before being recorded (`../evidence/BV0A/amd-008-round10-reviews.md`).
Two were self-inflicted by the round-9 fix and are closed by this revision:
the seed vector artifact's V4 decided a layer-1 chaining/table-policy question
ahead of the freeze the artifact's own rules require, now explicitly flagged
provisional in place; and this section's own prior sentence describing the
cut's rationale had reverted to the pre-split "vector artifact and the
implementation's own acceptance review" framing, corrected above. One is a
supersession-completeness gap closed by this revision: AMD-007:51-52 ("does
not reopen BF2 or let the candidate generate or modify its own oracle") was
quoted but not enumerated in §4's exhaustive list; §4 now states explicitly
that it is compatible and not superseded.

The fourth — architecture findings 2 and 3, and governance finding 1,
independently converging on the same root — is genuine design residue on the
new mechanism, not a drafting defect: layer 1's enumerated payload is
narrower than the umbrella sentence describing it, and the freeze process
proves neither exhaustive semantic coverage nor non-retroactive chronology
without a maintainer-adopted lock record. This is the recursive form of the
same "candidate cannot choose its own pass criteria" problem this program
already ruled on for BF2's performance gate. Ten review rounds, stable
unchallenged thesis since round 3, and a problem that further prose cannot
fully close (proving chronology for an artifact that does not yet exist is a
process property, not a text property) are the basis for disposing this
finding as **DEFER** rather than opening an eleventh round: recorded as debt
at [`../evidence/BV0A/debt-layer1-gate-authority.md`](../evidence/BV0A/debt-layer1-gate-authority.md)
(acceptance ID `FC-VUE-003`), owned by BV0A's own acceptance review, which is
the only point where real layer-1/reference/production commit identities
exist to be checked against each other for the completeness and chronology
this finding names. The maintainer's ratification of this amendment is the
recorded decision to accept that deferral.

Round 11's final independent three-way review closed conformance and
architecture `PASS`, and found two further governance findings on the debt
record itself: its chronology check was assess-only rather than
disqualifying (weaker than the BF2 precedent it claimed to match), and §5.1
overclaimed layer-1 "adoption" as already accomplished rather than
prospective future work gated on `FC-VUE-003`. Both fixed: the debt record's
chronology check now explicitly forbids reusing any pre-freeze prototype
(including anything traceable to the superseded `work/bv0a-implementation`
candidate) as evidence for the post-freeze work, matching the BF2 precedent's
phrasing; this section's wording above was corrected to describe adoption as
prospective. Round 12 (governance, targeted) confirmed both fixes and found
no further issue.

**Reviewed package:** commit `d75a6f79f34736d0347de69470a367b43d0bbeb7`, tree
`54f34b50b108582aed54975f2eb174881aa6ba92` — round 12's exact candidate, the
last content reviewed. **Ratification-bundle commit:**
`a1f6523ce752db969e19e073cb0b21c5a038e9a1`, same tree
`54f34b50b108582aed54975f2eb174881aa6ba92` — the twenty WIP commits accumulated
across all twelve review rounds squashed to one landing commit, tree
byte-identical to the reviewed package, so no landing-equivalence proof is
required. Full pre-squash history is preserved at branch
`backup/pre-amd008-squash`.

> Ratify AMD-008 for reviewed package commit
> `d75a6f79f34736d0347de69470a367b43d0bbeb7`, tree
> `54f34b50b108582aed54975f2eb174881aa6ba92`, and ratification-bundle commit
> `a1f6523ce752db969e19e073cb0b21c5a038e9a1`, tree
> `54f34b50b108582aed54975f2eb174881aa6ba92` (identical tree — no landing
> equivalence proof required); confirm BF2's accepted oracle and invocation
> are unchanged and unreopened, and that BV0A's owned-scope boundary (no
> identifier rename, no B3/B4/BV1/B5 authority) is otherwise unchanged;
> redefine BV0A's acceptance boundary to the exact §2 ordered map-artifact
> equality specification against an independent input-only reference; make
> `CodeTransform` semantics normative for both authorized rewrites per §2 item
> 5; establish the two-layer composition specification (frozen semantic layer
> plus literal vector coverage layer) named in §2 item 1 as the normative
> specification of the composition algebra, with the complete layer-1 freeze
> and layer-2 suite both reviewed BV0A acceptance deliverables rather than
> conditions of this ratification; reinforce BV0's literal full-oracle-clean
> exit per §3; accept the layer-1 gate-authority chronology finding as
> recorded debt (`FC-VUE-003`,
> `evidence/BV0A/debt-layer1-gate-authority.md`), owned by BV0A's own
> acceptance review, rather than pursue further amendment iteration.

**Maintainer decision: RATIFIED**, as recorded via the conversational exchange
with the program orchestrator on the disposition of the one remaining
substantive finding (accept as recorded debt rather than pursue a further
fix-and-review cycle) — the explicit basis for closing this amendment after
twelve review rounds.
