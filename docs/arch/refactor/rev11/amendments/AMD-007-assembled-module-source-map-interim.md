# AMD-007 — Interim Vue assembled-module source map

**Status:** PROPOSED — NOT RATIFIED. This draft has no execution authority.
**Prepared against:** local `program/architecture-lock` — commit/tree identities to be
regenerated once BF2 reopen #4 (see §1) lands and this draft's dependent text is
finalized against it.
**Amends on ratification:** [`../program-dag.toml`](../program-dag.toml), the live
program ledger, and the [`BV0.md`](../charters/BV0.md) charter; introduces the
inline `BV0A` charter in [§3](#3-bv0a-charter). It does not amend the B4, BV1,
or B5 charters or the emitter/mapping disposition ledger.

## 1. Binding direction and product boundary

[`BF2` reopen #3](../evidence/BF2/BF2-reopen3-summary.md) repaired the
official-oracle invocation defect. During BV0 relanding, a fresh source-map
comparison exposed a separate production gap: 18 of the exact 36 seed cells cannot
reach their mapping verdict because
`verter_session::compile::assemble_vue_main_module` returns a bare `String`. The
shipped Vue main-module assembler produces no assembled-module source map at all
for any oracle to inspect.

Separately, and discovered while drafting this amendment: BF2's existing mapping
axis (`packages/framework-conformance-harness/src/compare.mjs`, `compareMappings`/
`mappingsFieldEqual`) compares the candidate's source map against the official
Vue compiler's own generated map, field-by-field on decoded segments. Binding
maintainer direction supersedes that design: *"we do not need to golden sourcemap
from vue or sveltejs, we just need to guarantee that the sfc code that the user
writes is mapped correctly, nothing to do with vue or svelte official compiler."*
Verter's assembled module legitimately differs in structure from the official
compiler's own assembly, so matching its map shape is not a valid correctness
oracle — it can reject correct Verter output and accept wrong output that merely
resembles the official map. This is a genuine BF2-owned defect (its charter never
mandated official-map equality — see `BF2.md:20/28`, "diagnostics, source-map, and
TypeScript-observable product validation" — the official comparison was an
implementation choice that exceeded the charter), requiring **BF2 reopen #4**:
replace the mapping axis with a self-referential oracle that validates the
candidate's source map against the exact authored SFC content the user wrote
(source identity, per-segment truthfulness, bidirectional round trips for
required anchors, correct assembly-composition translation, and no fabricated
provenance over synthetic/framework-injected bytes). Full design in the reopen #4
evidence once landed. BF2's charter itself needs no amendment — the correction is
implementation-level.

This amendment (AMD-007) does not proceed to review or ratification until BF2
reopen #4 is landed and reaccepted. Everything below assumes that reoccurs and
describes the state AFTER it.

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
against the exact authored SFC the user wrote, not against any official compiler's
own map shape — for the script-plus-template composition consumed by BV0's exact
seed matrix. It does not own B4's logical-source-unit model, general fragment
placement, canonical map-request policy, atomic artifact-set publication, or final
assembly architecture. It does not create a universal IR or a cross-framework
assembler.

BF2 (post reopen #4) remains the accepted owner of official compiler invocation,
goldens, normalization, structural/runtime/diagnostic comparison, and the
authored-source mapping oracle. BV0A supplies the missing candidate artifact to
that oracle; it does not reopen BF2 again or let the candidate generate or modify
its own oracle.

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
matrix can perform its required candidate-to-authored-SFC mapping validation after
harness artifacts are removed. "Correct" means: every generated position that
carries source provenance traces back to the exact location in the SFC the user
actually wrote, per BF2's authored-source oracle — never a comparison against any
official compiler's own map. Preserve every existing code, parse, link, runtime,
server, diagnostic, route, and performance contract.

### Owned scope

BV0A owns the current
`verter_session::compile::assemble_vue_main_module` source-map gap and independently
authored controls for that exact gap. It expressly owns:

1. replacing the bare-`String` production assembly result with one narrow Vue-only
   typed result that couples the assembled module code to its assembled source map;
2. composing the existing script and template output mappings through their exact
   placement and through every current script rewrite performed by the assembler,
   using `CodeTransform` for generated-code mutations and the same authoritative
   chunk walk that produces the final code;
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
5. correcting only assembly-owned mapping failures in the exact 36-cell seed domain
   and in the minimum independently authored positive, negative, and mutation
   controls required to prove the whole assembly-composition class.

The code and map are one result of one production assembly implementation. BV0A may
reuse the current generated-chunk/multi-source map-composition primitive when it
satisfies this charter, but this interim choice does not preselect B4's final
representation. It must not concatenate code and reconstruct offsets afterward,
scan generated output for placement, maintain a second map-only assembly path, or
fabricate mappings for synthetic bytes.

BV0A does not introduce B3's canonical request or map-policy model; B4's logical
source units, stable identity system, generic fragment contract, source-space
architecture, atomic artifact set, or publication transaction; BV1's complete Vue
semantic/conformance train; B5's sole direct compiler core; a universal or
cross-framework IR; a Svelte path; IDE, TSC, declaration, style-content, or custom
block mapping expansion; an official-compiler production dependency; or a
known-divergence, waiver, tracker, retraction, or typed refusal for an already
successful Vue route.

BV0A does not change BF2's oracle invocation, official goldens, structural/runtime/
diagnostic comparison semantics, or the authored-source mapping oracle itself
(reaccepted under reopen #4) — it consumes that oracle, it does not redefine it. It
does not take ownership of residual script-emitter or template-emitter mapping
defects that become visible after correct final assembly; those remain BV0 defects
within the exact seed domain.

### Required procedure

First add an independently authored regression that fails because the genuine
production assembler returns code without an assembled map. Record the exact 18
affected seed cells and prove the failure occurs before an authored-source mapping
verdict can even be attempted — an absent map is a hard failure, not a skip or a
not-applicable result.

Inventory every byte-producing operation in the current assembler and classify each
range as composed script, composed template, or synthetic assembly. Implement the
minimum single-path typed code-plus-map result. All code mutations must use the same
structured edit/chunk operations from which mapping geometry is derived; no post-hoc
string replacement, line padding, final-text scan, or separately maintained offset
table is acceptance-capable.

Compose the existing input maps rather than treating generated fragment positions as
authored Vue positions. Missing, malformed, ambiguous, or uncomposable required input
mapping is a hard implementation and acceptance failure; BV0A cannot land until it
is corrected within scope or returns `RESCOPE_REQUIRED`. It is never converted to an
empty map, approximate line-only map, decode/bounds-only validation without
authored-source provenance, or unmapped successful comparison.

Add independently authored controls covering at least one authored script anchor and
one authored template anchor in the same assembled module, multiline placement,
non-ASCII authored content before an anchor, JavaScript and TypeScript script input,
VDOM and Vapor client output, SSR output, and inline/non-inline applicability where
the seed matrix requests a map. Add negative controls proving synthetic
prelude/epilogue bytes do not claim authored provenance. Plant mutations for an
absent map, shifted generated line, shifted source column, dropped template segment,
dropped script segment, and wrong source identity; each must be detected by BF2's
real authored-source mapping oracle (not any candidate-versus-official comparator —
that mechanism was removed in BF2 reopen #4).

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
authored SFC for script and template anchors in the assembled module: every
source-bearing segment resolves to the correct authored source identity and exact
original span (exact-text or explicit authored semantic relation — an arbitrary
in-bounds mapping does not satisfy this), and required anchors pass bidirectional
round trips (generated→source and source→generated). Synthetic assembly scaffolding
carries no source-bearing mapping and does not acquire fabricated authored
provenance. All required input mappings are composed through exact generated
placement; no raw offset crosses a source-space boundary.

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
cross-framework IR, a new public product contract, or a change to BF2's reaccepted
authored-source mapping oracle beyond what BV0A's own composition work requires.

Stop if an input script or template map is absent, false, or uncomposable for reasons
outside current final assembly and the gap cannot be corrected without taking over a
successor's architecture. Do not invent source identity, approximate an exact
mapping, silently omit a required segment, broaden into a fragment-emitter rewrite,
or defer the literal BV0 exit while reporting BV0A complete.

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
   the exact authored SFC content — not official-compiler map equality. An absent
   map, decode/bounds-only check without authored-source provenance, skipped
   comparison, future-B4 promise, or BV0A debt row cannot satisfy that exit; and
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
result; neither may reintroduce an absent map, skipped comparison, or corrected
mapping defect.

## 6. Program-state transition and landing scope

This amendment's predecessor is BF2 **after reopen #4** (not reopen #3) — the
mapping-oracle correction described in §1 is a genuine BF2 defect fix, and BF2's
acceptance identity changes accordingly. BF3 and all of its retained authority are
unchanged by this amendment.

On ratification, the DAG and both tracked program-state shapes contain 58 identical
block IDs. The new BV0A row is exposed as `READY` once BF2 reopen #4 is accepted.
BV0 returns to `LOCKED` until BV0A is accepted; its invalidated round-one reviews do
not revive and no prior BV0 candidate gains authority. After BV0A acceptance, BV0 may
be exposed as `READY` for a fresh candidate and fresh three-mandate review.

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

AMD-005's exact compatibility domains, official-core oracle and exclusion rules,
capability matrix, conformance normalizer, fragment/assembly final contract, and
performance-lock process remain in force unchanged for every axis OTHER than
mapping — official compiler oracles remain authoritative for structural, runtime,
diagnostic, and link conformance. Only mapping correctness is, and remains, judged
against the exact authored SFC rather than any official compiler's own map.

## 8. Exact ratification action

After the amendment package, proposed DAG and program-state updates, BV0 charter
delta, inline BV0A charter, and independent architecture/conformance/governance
reviews bind one exact candidate commit and tree — prepared against BF2 **post
reopen #4** — the designated maintainer records:

> Ratify AMD-007 for reviewed package commit `<reviewed-full-sha>`, tree
> `<reviewed-tree-oid>`, and ratification-bundle commit `<bundle-full-sha>`,
> tree `<bundle-tree-oid>`; confirm that BF2's accepted oracle, goldens,
> harness-artifact removal, and reaccepted authored-source mapping oracle (reopen
> #4) remain unchanged; authorize BV0A as the minimum interim owner of the genuine
> production Vue assembled-main-module code-plus-map result; amend the DAG to
> `BF2 -> BV0A -> BV0` while leaving BF3 and the B2/B3 joins otherwise
> unchanged; require BV0A acceptance before BV0 acceptance and retain BV0's
> literal 36/36 exit, judged against the authored-SFC mapping oracle, not
> official-compiler map equality; preserve EM-038's final `Replace` disposition
> and the complete BV1+B4+B5 cutover authority; expose BV0A, and no B2/B3/B4
> successor, to dispatch after ratification.

Silence, review, merge, or this proposal's commit is not ratification. Any changed
reviewed-package byte requires regenerated identities and fresh reports. The
preparer cannot ratify, review, or satisfy any independent mandate.

### 8.1 Recorded ratification

**PENDING MAINTAINER RATIFICATION.** Additionally pending: BF2 reopen #4 (§1) must
land and be reaccepted before this draft's identities are regenerated and its
independent reviews are dispatched.

This subsection is reserved for the exact reviewed package and tree, the three closed
independent review verdicts, the ratification-bundle identity, and the designated
maintainer's recorded action. Until those facts replace this stub, AMD-007 has no
execution authority.
