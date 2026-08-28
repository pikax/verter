# AMD-007 — Interim Vue assembled-module source map

**Status:** RATIFIED (see §8.1). Landed at `acabec8fa`.
**Prepared against:** local `program/architecture-lock` commit
`0519d92f325481e4fd1c52ba6d73f785769ce384`, tree
`d06a2a5a351206bd90f5877a1ea3c5625da2e099` (post BF2 reopen #4 acceptance —
`evidence/BF2/reopen4/landing-record.md`).
**Amends on ratification:** [`../program-dag.toml`](../program-dag.toml), the live
program ledger, and the [`BV0.md`](../charters/BV0.md) charter; introduces the
inline `BV0A` charter in [§3](#3-bv0a-charter). It does not amend the B4, BV1,
or B5 charters or the emitter/mapping disposition ledger.

## 1. Binding direction and product boundary

BF2 reopen #4 is already landed and reaccepted. Its authored-source mapping oracle
reads the authored fixture from disk and validates one candidate's map against that
candidate's own generated code: source identity and bounds, each source-bearing
segment's declared relation, exact required-anchor starts plus authored-span
coverage, and absence of authored provenance over generated-only ranges. Historical
note: reopen #4 replaced an earlier candidate-versus-official mapping comparison;
that superseded design is context only and is not part of BV0A's contract.

With the accepted oracle in place, BV0 relanding exposed a separate production gap:
18 of the exact 36 seed cells cannot reach their authored-source mapping verdict
because `verter_session::compile::assemble_vue_main_module` returns a bare `String`.
The shipped Vue main-module assembler produces no assembled-module source map for
the oracle to inspect.

BV0's charter requires correction of source-map defects after BF2's harness
artifacts are removed, but its abort boundary also forbids introducing B4's
publication architecture. B4, BV1, and B5 are still locked. BV0 therefore cannot
truthfully satisfy its ratified exit and cannot lawfully absorb the missing
assembly capability itself.

On ratification, `BV0A` — **Interim Vue assembled-module source map** — is the sole
new block. The ID is deliberately adjacent to BV0: it identifies a bounded enabling
slice for the immediate Vue correction train, preserves every ratified block ID,
and does not present the work as an early BV1 conformance train or an early B4
publication cutover.

BV0A owns only the minimum production seam needed to make the current shipped Vue
main-module assembler return its assembled code with a real, correct map — correct
under BF2's accepted authored-source contract for the script-plus-template
composition consumed by BV0's exact seed matrix. It does not own B4's
logical-source-unit model, general fragment placement, canonical map-request policy,
atomic artifact-set publication, or final assembly architecture. It does not create
a universal IR or a cross-framework assembler.

BF2 (post reopen #4) remains the accepted owner of framework-compiler invocation,
goldens, normalization, structural/runtime/diagnostic comparison, and the
authored-source mapping oracle. BV0A supplies the missing candidate artifact to
that oracle; it does not reopen BF2 or let the candidate generate or modify its own
oracle.

## 2. Amended DAG

The amended region is:

```text
B1 -> BF1 -> BF2 -> {BV0A, BF3}
BV0A -> BV0
{BV0, BF3} -> {B2, B3}
{B2, B3} -> B4
B4 -> {BV1, BS1}
{BV1, BS1} -> B5 -> B6
```

On ratification, the landing package adds this machine-readable row to
[`../program-dag.toml`](../program-dag.toml):

```toml
[[block]]
id = "BV0A"
name = "Interim Vue assembled-module source map"
class = "subsystem"
predecessors = ["BF2"]
```

The existing BV0 row becomes:

```toml
[[block]]
id = "BV0"
name = "Immediate Vue known-defect correction"
class = "subsystem"
predecessors = ["BV0A"]
```

The BF3, B2, and B3 predecessor rows remain otherwise unchanged. In particular,
BF3 remains a direct BF2 successor and B2/B3 continue waiting for both BV0 and BF3.
BV0A must be accepted before BV0 can be accepted. Contingent BV0 development may be
restacked above a BV0A candidate under the existing governance rules, but it creates
no permission to review, land, or accept BV0 before the new predecessor lands and is
accepted.

BV0A and BF3 may overlap only after an exact writable-ownership proof demonstrates
disjoint production files, tests, generated artifacts, manifests, and lockfiles.
Without that proof they serialize.

## 3. BV0A charter

On ratification, the following charter is ratified verbatim. It is inline because
this amendment is a proposal and does not create a separate charter file.

**Status:** PROPOSED / LOCKED. **Class:** Framework subsystem.
**Predecessor:** BF2 (post reopen #4).

### Objective

Add the minimum Vue-only production capability that assembles the current runtime
main module and a correct source map together, so BV0's exact 36-cell BF2 seed
matrix can perform its required authored-source mapping validation after harness
artifacts are removed. "Correct" means that the assembled map satisfies BF2's
accepted authored-source oracle's source-identity, per-segment relation,
required-anchor, and generated-only-range rules. Preserve every existing code,
parse, link, runtime, server, diagnostic, route, and performance contract.

### Owned scope

BV0A owns the current
`verter_session::compile::assemble_vue_main_module` source-map gap and independently
authored controls for that exact gap. It expressly owns:

1. replacing the bare-`String` production assembly result with one narrow Vue-only
   typed result that couples the assembled module code to its assembled source map;
2. composing the existing script and template output mappings through their exact
   placement in one forward pass through the assembler's existing write order,
   shifting each sub-map by the running generated-line cursor at the point its code
   is written, and handling the known `__sfc__`-to-`_sfc_main` rename (7-to-9-byte,
   requiring a per-occurrence generated-column delta on the retained line) and the
   real GLOBAL (not suffix-only) `export default _sfc_main;` removal (requiring full
   line-and-column splice geometry per occurrence — a mid-line match joins the
   following line onto its prefix, not merely a line-count shift) as direct bounded
   operations over the already-in-hand script string; `CodeTransform` remains an
   optional fallback only for a genuinely more complex rewrite discovered during
   implementation, not a required representation for these two known rewrites. An
   equal-byte-length placeholder rename was investigated and rejected: `__sfc__` is
   not purely internal to the session assembler — it is a cross-consumer protocol
   string also produced as the IDE Options API path's own public default
   (`ide/script/options_api.rs`, never passing through the session assembler) and
   recognized by name in the playground's own compiler (`packages/playground/src/core/compiler.ts`).
   Renaming it changes observable behavior on paths BV0A does not own;
3. preserving declared source identities, source contents, lines, columns, and
   segment order across the script and template portions of the final module, while
   leaving assembly-only imports, render attachment, custom-block invocation,
   `__file`, HMR, SSR-context, and export scaffolding unmapped unless a genuine
   authored-source mapping exists; source-map coordinates remain 0-based V3
   line/column positions and typed source/generated byte positions are converted at
   their owning boundary rather than mixed as raw offsets;
4. making the map-enabled result from the genuine shipped production assembler
   available to BV0's existing BF2-backed seed-matrix candidate path, validated
   through BF2's reaccepted authored-source oracle, with no harness copy or
   synthetic candidate map; and
5. correcting only mapping failures rooted in the exact byte-producing operations
   inside the current `assemble_vue_main_module` implementation, as exercised by the
   named 36 seed cells and the required independently authored positive, negative,
   and mutation controls.

The code and map are one result of one production assembly implementation. BV0A may
reuse an existing multi-source map-composition primitive when it satisfies this
charter, but it does not require a chunk IR and this interim choice does not
preselect B4's final representation. It must not concatenate code and reconstruct
offsets afterward, scan generated output for placement, maintain a second map-only
assembly path, or fabricate mappings for synthetic bytes. The required running
generated-line cursor is compliant with this prohibition: it is updated during
each write from that write's known newline contribution and is never recovered by
inspecting or reparsing the assembled result.

BV0A does not introduce B3's canonical request or map-policy model; B4's logical
source units, stable identity system, generic fragment contract, source-space
architecture, atomic artifact set, or publication transaction; BV1's complete Vue
semantic/conformance train; B5's sole direct compiler core; a universal or
cross-framework IR; a Svelte path; IDE, TSC, declaration, style-content, or custom
block mapping expansion; a framework-compiler production dependency; or a
known-divergence, waiver, tracker, retraction, or typed refusal for an already
successful Vue route.

BV0A does not change BF2's accepted authored-source mapping oracle or its invocation,
nor BF2's goldens or structural/runtime/diagnostic comparison semantics. This limit
is unconditional: BV0A consumes the oracle and does not redefine it. It does not take
ownership of residual script-emitter or template-emitter mapping defects that become
visible after correct final assembly; those remain BV0 defects within the exact seed
domain.

### Required procedure

First add an independently authored regression that fails because the genuine
production assembler returns code without an assembled map. Record the exact 18
affected seed cells and prove the failure occurs before an authored-source mapping
verdict can even be attempted — an absent map is a hard failure, not a skip or a
not-applicable result.

Inventory every byte-producing operation in the current assembler and classify each
range as composed script, composed template, or synthetic assembly. Implement the
minimum single-path typed code-plus-map result as one forward pass in the function's
existing write order. That order is: style and custom-block imports plus their
conditional separator; template runtime and SSR-helper imports; script output or the
synthetic no-script `_sfc_main` and optional scope-id assignment; the blank line,
template output, terminating newline when needed, and immediate `render` or
`ssrRender` attachment; custom-block invocations; development `__file`; client HMR;
the SSR-context block; and the final export default. In particular, the real
function writes render attachment immediately after the template, while the
SSR-context `useSSRContext` import is part of the late SSR block rather than the
initial import prelude. Preserve those placements and the existing template-fragment
inspection that selects `render` versus `ssrRender`; do not replace it with a scan of
the finished module.

Maintain a 0-based running generated-line cursor initialized before the first write.
Each append updates the cursor from the exact newline count contributed by that write.
Immediately before writing a mapped script or template fragment, capture the cursor
as that fragment's start line and compose its map by adding only that start-line
offset to every generated line. Preserve the current synthetic newline writes that
make mapped fragments start at column zero. Synthetic prelude, separator, fallback,
attachment, and epilogue writes advance the cursor but contribute no mappings. This
write-time scalar cursor is the placement authority; no line padding, final-text
scan, reparse of the assembled result, or separately maintained offset table is
acceptance-capable.

Before the script fragment is written, handle its two current rewrites directly,
each over the already-in-hand script string (bounded, known-content scanning of one
intermediate piece BEFORE it is written, not scanning the finished assembled
module — compliant with the placement-authority rule above). First, the whole-string
`__sfc__`-to-`_sfc_main` rename (7-to-9 ASCII bytes): for every occurrence, apply a
+2-column shift to every mapped segment on the same generated line at or after the
occurrence's end column, cumulative for multiple occurrences on one line, with no
line-count change (the rename never inserts or removes a newline) and no change to
source coordinates. Second, the export removal is a GLOBAL, not suffix-only,
replacement of every literal `export default _sfc_main;\n` occurrence — including
one that could appear inside authored content (a comment, a multiline template
literal) ahead of the compiler-emitted trailing one, not only the final line. Model
this faithfully: find every occurrence, and for each, splice both the string and its
map as a real removal, not a line-count-only shift — a match starting at column
`c > 0` joins the following line onto that prefix (a segment formerly at
`(line + 1, k)` moves to `(line, c + k)`, not `(line, k)`), so apply the full
line-and-column splice geometry per occurrence, cumulatively, and drop any mapping
that fell inside the removed range. If the implementer instead prefers to change the
real assembler to a genuinely suffix-only removal (the far more common case in
practice, and arguably simpler), that is a candidate production behavior change
outside a pure map-composition fix and must be dispositioned explicitly
(architecture ruling, and the "byte-unchanged" required-exit language in this
document updated to reflect it) rather than silently assumed.
Compose that adjusted script map with the write-time line offset above; compose the
unmodified template map the same way when the template is written. These two
structurally known cases do not require `CodeTransform` or a general edit/chunk
abstraction. If the byte-operation inventory finds another rewrite whose mapping
geometry is genuinely more complex, the implementer may use `CodeTransform` for
that rewrite, provided it still drives the same single code-and-map path and does
not derive placement from the finished text. No mutation applied after a fragment
is written and no second map-only assembly path is acceptance-capable.

Compose the existing input maps rather than treating generated fragment positions as
authored Vue positions. Missing, malformed, ambiguous, or uncomposable required input
mapping is a hard implementation and acceptance failure. BV0A may correct it only
when its root cause is one of the exact byte-producing composition operations owned
above; an oracle-side or invocation root cause returns `RESCOPE_REQUIRED` to BF2,
and a script-emitter or template-emitter root cause returns `RESCOPE_REQUIRED` to
BV0. It is never converted to an empty map, approximate line-only map,
decode/bounds-only validation without authored-source provenance, or an unmapped
successful result.

Add independently authored controls covering at least one authored script anchor and
one authored template anchor in the same assembled module, multiline placement,
non-ASCII authored content before an anchor, JavaScript and TypeScript script input,
VDOM and Vapor client output, SSR output, and inline/non-inline applicability where
the seed matrix requests a map. Add negative controls proving synthetic
prelude/epilogue bytes do not claim authored provenance. For every assembly-owned
synthetic or scaffolding range that BV0A's new code introduces, bind its exact
generated range to an explicit non-loose discriminator in a BV0A-owned independent
control through the accepted oracle's `extraSyntheticRanges` parameter, or use
another independently authored control with equivalent discrimination. A fabricated
source-bearing segment on any such range must fail even if its authored position
would otherwise satisfy
`component-instance-surface`, `framework-emitted-token`, or `delimiter-anchor`; this
obligation does not authorize a change to the oracle or its invocation. Plant
mutations for an absent map, shifted generated line, shifted source column, dropped
template segment, dropped script segment, wrong source identity, and a fabricated
source-bearing segment in each such BV0A-introduced range; each must be detected by
BF2's accepted authored-source mapping oracle.

For every new correctness-bearing test, persist a reversible mutation recipe that
records the starting identity, proves the plant was applied, requires the named RED
result, restores and verifies the original identity, requires GREEN, and runs an
unplanted control that stays GREEN. The independent confirmer reruns every recipe;
sampling is not acceptance evidence.

Run BF2's authored-source mapping oracle after its already accepted harness-artifact
removal. Then rerun every affected seed axis plus the full 36-cell BV0 matrix in
authoritative fail-closed mode. Prove existing assembled code bytes and all
previously passing parse, exact-package-link, structure, runtime/server, diagnostic,
and route results remain unchanged. Rerun the applicable locked performance cells
without changing their thresholds. On the final frozen candidate, run the canonical
Rust gate and the affected BF2 harness/conformance suites with their pinned local
closure; a skipped required axis, zero-test run, timeout, incomplete run, or
network-fetched oracle is not a pass.

### Required exits

`assemble_vue_main_module` no longer terminates at a bare `String`; the genuine
production assembly path exposes one typed assembled code-plus-map result and every
map-enabled caller observes the map paired with the exact code from which it was
generated. No harness-only candidate map or duplicate assembly implementation exists.

The source map is non-empty, decodable, deterministic, and correct against the exact
authored SFC for script and template anchors in the assembled module. Every declared
source resolves to that fixture, any declared source content matches its on-disk
bytes, and every source-bearing segment is in bounds and satisfies one of the
oracle's declared relations. The six position-exact relations are `verbatim-carry`,
`context-binding-prefix`, `macro-result-binding`, `event-handler-key`,
`synthesized-local-for-authored-name`, and `destructured-binding-pattern` (with
`verbatim-carry` exact only up to identical-lexeme interchangeability).
`component-instance-surface`, `framework-emitted-token`, and `delimiter-anchor` are
intentionally loose on the authored side and require only an in-bounds,
non-word-interior authored position after their strong generated-side precondition
is met. Required anchors satisfy the oracle's actual completeness checks: a segment
exists at the exact authored anchor start, the authored anchor span has segment
coverage, and an exact-start segment uses an allowed relation. These checks do not
claim a true inverse lookup back to the same generated coordinate.

Synthetic assembly scaffolding carries no source-bearing mapping and does not
acquire inherited authored provenance. Every assembly-owned synthetic or scaffolding
range introduced by BV0A's new code is bound to `extraSyntheticRanges` or an
equally non-loose independently authored discriminator, and a fabricated
source-bearing segment on any such range fails regardless of whether one of the
three loose relations could classify its authored position. All required input
mappings are composed through exact generated placement; no raw offset crosses a
source-space boundary.

For all 36 exact BV0 seed cells, every applicable mapping check genuinely invokes
BF2's reaccepted authored-source mapping oracle after harness artifacts are removed.
The 18 cells blocked by the absent assembled map no longer fail for absence,
malformation, assembly offset, source identity, or script-plus-template composition.
No mapping check is skipped, treated as not applicable because the candidate lacks a
map, or replaced by decode/bounds-only validation without authored-source
provenance. Every planted mutation is detected.

All pre-existing assembled-code bytes and successful parse, link, normalized
structure/helper-topology, deterministic runtime/server, diagnostic, and public-route
results remain unchanged. Applicable locked performance cells remain within their
existing thresholds. The change is Vue-only and adds no B3/B4/BV1/B5 authority, no
universal IR, no alternate production assembly route, and no waiver or deferral
artifact.

### Abort/rescope

Stop with `RESCOPE_REQUIRED` if a correct map for the exact bounded seed domain
requires B3's canonical request, B4's general logical-unit/identity/publication
architecture, BV1's complete Vue plan, B5's direct-core cutover, a universal or
cross-framework IR, a new public product contract, or any change to BF2's accepted
authored-source mapping oracle or its invocation. Oracle and invocation immutability
is unconditional; BV0A has no exception for a change its composition work appears to
require.

Stop with `RESCOPE_REQUIRED` if an input script or template map is absent, false, or
uncomposable for any root cause outside the exact byte-producing operations in the
current `assemble_vue_main_module` implementation. Rescope oracle-side or invocation
issues to BF2 and script-emitter or template-emitter issues to BV0; never correct
either opportunistically inside BV0A. Do not invent source identity, approximate an
exact mapping, silently omit a required segment, broaden into a fragment-emitter
rewrite, or defer the literal BV0 exit while reporting BV0A complete.

## 4. BV0 charter amendment

On ratification, [`BV0.md`](../charters/BV0.md) is amended in five bounded ways:

1. its predecessor changes from BF2 to BV0A;
2. its owned source-map scope is clarified so BV0A owns existence and composition of
   the current final assembled-module map, while BV0 retains any residual
   script-emitter, template-emitter, or Vue-semantic mapping correction exposed by
   the accepted assembled map in the exact seed domain;
3. its required procedure must consume the genuine BV0A production code-plus-map
   result through BF2's reaccepted authored-source mapping oracle and may not
   synthesize a candidate map in the harness;
4. its required exits remain literal: all 36 exact seed cells must pass every
   applicable mapping check after harness artifacts are removed, validated against
   the exact authored SFC content under the accepted oracle's source-identity,
   per-segment relation, required-anchor, and generated-only-range rules. An absent
   map, decode/bounds-only check without authored-source provenance, skipped required
   mapping check, future-B4 promise, or BV0A debt row cannot satisfy that exit; and
5. its "source-map differences after harness artifacts are removed" language
   (`BV0.md:20` in the pre-amendment text) is reworded to name the authored-source
   oracle explicitly: source-map correctness defects against the exact authored
   Vue SFC, including incorrect source identity/coordinates, missing required
   authored anchors, and fabricated provenance over synthetic output.

BV0's objective, named compiler corrections, preservation of successful public
routes, prohibition on guards/trackers/waivers, and remaining abort/rescope boundary
stay in force. This amendment narrows only the newly discovered assembly-capability
precondition and clarifies the mapping oracle's definition; it does not reduce BV0's
36-cell acceptance domain.

## 5. B4, BV1, B5, and EM-038 preservation

The [`B4.md`](../charters/B4.md), [`BV1.md`](../charters/BV1.md), and
[`B5.md`](../charters/B5.md) charters are not amended. Their final ownership remains:

- BV1 owns Vue semantic models, plans, script/template assembly semantics, and the
  complete accepted Vue mapping pack on the final substrate;
- B4 owns logical source units and identities, general fragment placement,
  source-space map composition, and atomic artifact publication; and
- B5 exposes the accepted framework algorithms through the sole direct compiler core
  and removes alternate publication routes.

The existing
[`EM-038`](../evidence/framework-conformance/emitter-mapping-dispositions.tsv) row
remains `Replace` with acceptance owner `BV1+B4+B5`. BV0A neither changes that row to
`Preserve`/`Converge` nor adds itself as a final acceptance owner. It authorizes a
temporary correctness repair to the current owner only because BV0 requires the map
before B4 is dispatchable.

EM-038 is therefore preserved and temporally narrowed, not contradicted: BV0A makes
the current session assembler map-capable for the bounded predecessor need; BV1+B4+B5
must still replace the host text-rewriting/final-module path with Vue-owned fragments,
B4 assembly/map composition and atomic publication, and the sole B5 direct core. No
second session assembly path survives that cutover.

BV1's existing BV0-preservation exit already requires the exact BV0 seed pack to
remain green on the final B2–B4 substrate. That obligation includes the
authored-source mapping correctness made executable by BV0A. BV1 or B4 may replace
the interim representation only with an accepted equivalent or stronger final
result; neither may reintroduce an absent map, skipped mapping check, or corrected
mapping defect.

## 6. Program-state transition and landing scope

This amendment's predecessor is BF2 at its already accepted post-reopen-#4 identity.
BF3 and all of its retained authority are unchanged by this amendment.

On ratification, the DAG and both tracked program-state shapes contain 58 identical
block IDs. Because BF2 reopen #4 is already accepted, the new BV0A row is exposed as
`READY` on ratification. BV0 returns to `LOCKED` until BV0A is accepted; its
invalidated round-one reviews do not revive and no prior BV0 candidate gains
authority. After BV0A acceptance, BV0 may be exposed as `READY` for a fresh candidate
and fresh three-mandate review.

This proposal intentionally changes no machine-readable DAG, program-state shape,
live ledger, emitter/mapping ledger, or standalone charter file. Those changes are
materialized only by a byte-exact ratification/landing package after the review and
maintainer action in [§8](#8-exact-ratification-action). The BV0A implementation is a
separate program block and cannot ride in this amendment draft.

## 7. Scope of amendment and supersession

On ratification, this amendment supersedes the direct `BF2 -> BV0` edge, the portion
of AMD-006 §3/BV0's charter that would otherwise appear to require BV0 to create the
missing current-assembler map despite its B4 abort boundary, and BV0's prior
"source-map differences" wording per §4.5 above. It does not supersede AMD-006's Vue
correction direction, BF3 narrowing, BV1 preservation rule, or literal 36-cell exit.

AMD-005's exact compatibility domains, capability matrix, conformance normalizer,
fragment/assembly final contract, and performance-lock process remain in force. Its
official-core oracle and exclusion rules remain authoritative for structural,
runtime, diagnostic, and link conformance. Mapping correctness remains governed by
BF2's accepted authored-source oracle over the exact authored SFC, candidate code,
candidate map, declared segment relations, required anchors, and generated-only
ranges.

## 8. Exact ratification action

After the amendment package, proposed DAG and program-state updates, BV0 charter
delta, and inline BV0A charter bind one exact candidate commit and tree — prepared
against BF2 **post reopen #4** — independent conformance, architecture, and
governance/adversarial reviews must each bind that exact candidate and record a
closed `PASS` verdict. If and only if all three reviews are `PASS`, the designated
maintainer records:

> Ratify AMD-007 for reviewed package commit `<reviewed-full-sha>`, tree
> `<reviewed-tree-oid>`, and ratification-bundle commit `<bundle-full-sha>`,
> tree `<bundle-tree-oid>`; confirm that BF2's accepted framework-compiler
> invocation, goldens, normalization, structural/runtime/diagnostic comparison,
> harness-artifact removal, and authored-source mapping oracle and invocation
> (reopen #4) remain unchanged; authorize BV0A as the minimum interim owner of the
> genuine production Vue assembled-main-module code-plus-map result; amend the DAG to
> `BF2 -> BV0A -> BV0` while leaving BF3 and the B2/B3 joins otherwise
> unchanged; require BV0A acceptance before BV0 acceptance and retain BV0's
> literal 36/36 exit under the authored-SFC mapping oracle's source-identity,
> per-segment relation, required-anchor, and generated-only-range rules; preserve
> EM-038's final `Replace` disposition and the complete BV1+B4+B5 cutover authority;
> expose BV0A, and no B2/B3/B4 successor, to dispatch after ratification.

Silence, review, merge, or this proposal's commit is not ratification. Any changed
reviewed-package byte requires regenerated identities and fresh reports. The
preparer cannot ratify, review, or satisfy any independent mandate.

### 8.1 Recorded ratification

Six independent review rounds ran against this proposal as it was corrected and
simplified; full history and dispositions at
[`../evidence/vue-known-defect-correction/amd007-review-history.md`](../evidence/vue-known-defect-correction/amd007-review-history.md).
Round 3 closed all three mandates `PASS` on the oracle-dependency/scope/governance
structure. Round 6 (final, focused) closed with one finding — a documented
REJECT-with-rationale for an investigated-and-abandoned byte-preservation rename,
misread as a "dangling reference" — dispositioned by the program orchestrator as
not substantive (the review's own math/logic checks otherwise all passed) rather
than spent on a seventh round. The maintainer additionally proposed, and the
program orchestrator investigated and found unsafe, two candidate byte-preservation
techniques for the `__sfc__`-to-`_sfc_main` rename (growing the internal placeholder
to 9 bytes; shrinking the target to a 7-byte `sfcmain` spelling) — both touch
identifiers used by production consumers outside BV0A's scope (the IDE Options API
path and playground for the former; `packages/unplugin`, the virtual-file pipeline,
and the Vue IDE bridge for the latter, plus 108 official golden records requiring
the exact `_sfc_main` spelling). The maintainer confirmed keeping the already
twice-independently-verified per-occurrence column-delta approach and proceeding to
ratification without either rename.

Reviewed package: commit `006b566d5b4d7a9c8a9c76c75389fb2339dc8cf0`, tree
`b000474a11565db1acdb309f9884f4608f98e516` (round 6's exact candidate, unchanged
since). Ratification-bundle commit `0068389858792c912420d6097cb04bd3106f7b5c`, tree
`80be54a72f6821502709a36823f679090a46148d` (adds only the review-history evidence
record on top; the amendment text itself is byte-identical to the reviewed
package).

> Ratify AMD-007 for reviewed package commit
> `006b566d5b4d7a9c8a9c76c75389fb2339dc8cf0`, tree
> `b000474a11565db1acdb309f9884f4608f98e516`, and ratification-bundle commit
> `0068389858792c912420d6097cb04bd3106f7b5c`, tree
> `80be54a72f6821502709a36823f679090a46148d`; confirm that BF2's accepted
> framework-compiler invocation, goldens, normalization, structural/runtime/
> diagnostic comparison, and authored-source mapping oracle (reopen #4) remain
> unchanged; authorize BV0A as the minimum interim owner of the genuine production
> Vue assembled-main-module code-plus-map result via a write-order running-cursor
> composition (no CodeTransform/chunk-IR mandate, no identifier rename); amend the
> DAG to `BF2 -> BV0A -> BV0` while leaving BF3 and the B2/B3 joins otherwise
> unchanged; require BV0A acceptance before BV0 acceptance and retain BV0's literal
> 36/36 exit under the authored-SFC mapping oracle's rules; preserve EM-038's final
> `Replace` disposition and the complete BV1+B4+B5 cutover authority; expose BV0A,
> and no B2/B3/B4 successor, to dispatch after ratification.

**Maintainer decision: RATIFIED**, as recorded via the conversational exchange
above this section's authoring (program orchestrator session, this date) — explicit
approval given after review of the six-round history and the two investigated
rename alternatives, with direction to keep the column-delta approach and proceed.
