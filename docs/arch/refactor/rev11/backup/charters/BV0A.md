# BV0A — Interim Vue assembled-module source map

**Status:** PROPOSED / LOCKED. **Class:** Framework subsystem.
**Predecessor:** BF2 (post reopen #4).

## Objective

Add the minimum Vue-only production capability that assembles the current runtime
main module and a correct source map together, so BV0's exact 36-cell BF2 seed
matrix can perform its required mapping validation after harness artifacts are
removed. "Correct" means: the assembled module's code is byte-identical to the
pinned pre-amendment baseline, its emitted map is a valid flat source-map v3
artifact, and that artifact equals an independently computed reference artifact
EXACTLY, field for field and position for position, including the exact ORDERED
sequence of segments (a multiset or sorted comparison is forbidden — segment order
is load-bearing: the accepted decoder preserves wire order, and the oracle's own
lookup selects the LAST applicable segment at or before a column, so reordering two
equal-coordinate segments changes which authored position a consumer resolves even
though it leaves any multiset identical). Authored `original` coordinates, source
spellings, and names are carried OPAQUELY and unchanged — whether they tell the
truth about the authored SFC is BV0's concern, not BV0A's; BV0A's objective is only
that composition neither invents, drops, reorders, duplicates, nor perturbs them.
Preserve every existing code, parse, link, runtime, server, diagnostic, route, and
performance contract.

**The normative specification of the composition algebra — the exact canonical
output schema, the exact chaining/transform algebra for both authorized rewrites,
and the exact rules for assembly-owned sourceless boundaries — is a separate,
independently reviewed and frozen artifact, not prose in this charter.** It has two
layers: a frozen SEMANTIC SPECIFICATION (the DTO schema, validation order/taxonomy,
chaining/collision policy, and an exhaustive assembler write/boundary-site
manifest), independently reviewed and frozen BEFORE either the independent
reference or the production implementation is written against it, changeable only
by amendment thereafter; and a literal VECTOR COVERAGE SET
(`packages/framework-conformance-harness/vectors/assembled-map-composition.vectors.json`),
completed and schema-bound as a BV0A acceptance deliverable, frozen at BV0A's
acceptance. Both the independent reference and the production Rust assembler must
reproduce every vector exactly. Where charter prose and the frozen specification or
vectors could be read to disagree, the frozen artifacts govern.

**A known, tracked gap in this specification's own gate authority is recorded as
debt, not a blocking condition of this charter:** whether the layer-1 freeze
genuinely predates, and was not derived from, any prototype implementation, and
whether layer 1's payload is exhaustively total over every semantic decision the
umbrella description above names, must be verified at BV0A's acceptance review per
the resolution gate at
[`../evidence/BV0A/debt-layer1-gate-authority.md`](../evidence/BV0A/debt-layer1-gate-authority.md)
(acceptance ID `FC-VUE-003`). A BV0A candidate cannot be accepted while any of that
resolution gate's three checks remains unmet.

## Owned scope

BV0A owns the current
`verter_session::compile::assemble_vue_main_module` source-map gap and independently
authored controls for that exact gap. It expressly owns:

1. replacing the bare-`String` production assembly result with one narrow Vue-only
   typed result that couples the assembled module code to its assembled source map;
2. composing the existing script and template output mappings through their exact
   placement, under `CodeTransform`'s LOCAL code-and-map semantics made NORMATIVE
   for the two authorized rewrites (this does not mandate a whole-module or
   cross-block chunk IR, and confers no B4 authority): pass one globally overwrites
   every `__sfc__` with `_sfc_main` (7 bytes to 9); pass two, on pass one's output
   coordinate space, globally removes every `export default _sfc_main;\n`. Token
   geometry follows `Chunk::Overwritten` — a non-empty overwrite emits one token at
   the replacement's generated start, mapped to the overwritten range's original
   start; an empty overwrite emits no replacement token. A bespoke offset/clamp
   formula over decoded positions is forbidden; the two rewrites are real
   `CodeTransform` transforms applied sequentially, each driving both output code
   and output map. Provenance (script/template/assembly-boundary origin) is tagged
   at ingestion and survives rewriting, placement, and table remapping as
   composition-time bookkeeping; it is never inferred from final coordinates or
   spelling, and the emitted wire map does not serialize it. An equal-byte-length
   placeholder rename was investigated and rejected: `__sfc__` is not purely
   internal to the session assembler — it is a cross-consumer protocol string also
   produced as the IDE Options API path's own public default
   (`ide/script/options_api.rs`, never passing through the session assembler) and
   recognized by name in the playground's own compiler
   (`packages/playground/src/core/compiler.ts`). Renaming it changes observable
   behavior on paths BV0A does not own. A second candidate byte-preservation
   technique (shrinking the rename target from `_sfc_main` to a 7-byte `sfcmain`
   spelling) was also investigated and rejected: `_sfc_main` is referenced by
   `packages/unplugin`, the virtual-file pipeline, and the Vue IDE bridge, and is
   the exact spelling 108 official golden records require under BF2's current
   identifier-structural comparison — neither rename is BV0A-scoped;
3. preserving declared source identities, source contents, lines, columns, and
   segment order across the script and template portions of the final module, while
   leaving assembly-only imports, render attachment, custom-block invocation,
   `__file`, HMR, SSR-context, and export scaffolding unmapped unless a genuine
   authored-source mapping exists; source-map coordinates remain 0-based V3
   line/column positions and typed source/generated byte positions are converted at
   their owning boundary rather than mixed as raw offsets;
4. making the map-enabled result from the genuine shipped production assembler
   available to BV0's existing BF2-backed seed-matrix candidate path, delivered to
   BF2's unchanged authored-source oracle with a recorded run for every cell (not
   a clean verdict — BF2's non-clean MAPPING verdict is excluded from BV0A's own
   gate; residual fragment-emitter violations are BV0's acceptance responsibility),
   with no harness copy (meaning no harness-synthesized BF2 candidate map and no
   duplicate production assembly route — the independent JavaScript reference item
   2 below mandates is a test-only oracle, never supplied to BF2 as a candidate,
   and is not what this prohibition forbids) or synthetic candidate map; and
5. correcting only mapping failures rooted in the exact byte-producing operations
   inside the current `assemble_vue_main_module` implementation, as exercised by the
   named 36 seed cells and the required independently authored positive, negative,
   and mutation controls.

The code and map are one result of one production assembly implementation. BV0A may
reuse an existing multi-source map-composition primitive when it satisfies this
charter, but it does not require a chunk IR and this interim choice does not
preselect B4's final representation. It must not concatenate code and reconstruct
offsets afterward, scan generated output for placement, maintain a second map-only
assembly path, or fabricate mappings for synthetic bytes.

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
domain. AMD-007's original sentence that BV0A "does not reopen BF2 or let the
candidate generate or modify its own oracle" remains live, compatible authority: the
JavaScript reference below is a test-only oracle that checks the candidate, never a
thing the candidate generates or modifies, and it is never supplied to BF2.

## Required procedure

First add an independently authored regression that fails because the genuine
production assembler returns code without an assembled map. Record the exact 18
affected seed cells and prove the failure occurs before a mapping verdict can even
be attempted — an absent map is a hard failure, not a skip or a not-applicable
result.

BV0A lands a test-only, independent, cross-language reference that computes the
expected map artifact from inputs alone, meeting three cumulative, structurally
auditable requirements:

- **Location and language.** The reference lives in the JavaScript conformance
  harness (`packages/framework-conformance-harness/`) with NO dependency — import,
  FFI, generated binding, or fixture produced by — on Rust composition, rewrite,
  placement, or map-emission code. A second Rust implementation beside the first
  does not satisfy this.
- **Input-only interface.** The reference consumes one serialized pre-assembly
  input DTO carrying every input the real assembler reads and nothing else — never
  the production map, splice lists, placement traces, or composition helpers.
- **No translation.** The reference is written from the frozen layer-1
  specification and the real `CodeTransform`/V3 semantics, not transcribed from
  the production implementation or its diff — the property that closes common-mode
  error between the two implementations, which is also why the vector suite
  requires literal hand-authored vectors produced by NEITHER implementation.

Acceptance evidence for a BV0A candidate requires: full conformance to the
complete frozen vector suite (every vector executed and reproduced exactly, with
the executed count asserted against the suite's own inventory); a comprehensive
positive fixture exercising real assembler geometry (both rewrites, mid-line and
terminal removal, equal-coordinate ordering, sourceless boundaries, astral/CRLF
text, duplicate table spellings); mutations proving the ordered-equality comparator
actually discriminates order, rewrite geometry, chain bias, placement, synthetic
provenance, and every compared artifact field individually; a fail-closed control
per `UncomposableInputMap` category (each failing at its own preflight validation
stage, before any artifact comparison runs); and a pinned-baseline control proving
a production-only code mutation produces a named RED at the CODE-baseline
assertion specifically. Each mutation must prove the plant was present, unique,
and new; the reference was unchanged where the mutation's own category does not
target it; the correct NAMED assertion for that mutation's category (comparator,
preflight, or baseline) produced the RED; and the original identity was restored
and reverified GREEN.

Missing, malformed, ambiguous, or uncomposable required input mapping is a hard
implementation and acceptance failure per the `UncomposableInputMap` taxonomy
below. BV0A may correct it only when its root cause is one of the exact
byte-producing composition operations owned above; an oracle-side or invocation
root cause returns `RESCOPE_REQUIRED` to BF2, and a script-emitter or
template-emitter root cause returns `RESCOPE_REQUIRED` to BV0.

For every new correctness-bearing test, persist a reversible mutation recipe that
records the starting identity, proves the plant was applied, requires the named RED
result, restores and verifies the original identity, requires GREEN, and runs an
unplanted control that stays GREEN. The independent confirmer reruns every recipe;
sampling is not acceptance evidence.

Run BF2's authored-source mapping oracle once per cell after its already accepted
harness-artifact removal, recording that it RAN for every cell. Then rerun every
affected seed axis plus the full 36-cell BV0 matrix. Prove existing assembled code
bytes and all previously passing parse, exact-package-link, structure,
runtime/server, diagnostic, and route results remain unchanged. Rerun the
applicable locked performance cells without changing their thresholds. On the
final frozen candidate, run the canonical Rust gate and the affected BF2
harness/conformance suites with their pinned local closure; a skipped required
axis, zero-test run, timeout, incomplete run, or network-fetched oracle is not a
pass.

## Required exits

`assemble_vue_main_module` no longer terminates at a bare `String`; the genuine
production assembly path exposes one typed assembled code-plus-map result and every
map-enabled caller observes the map paired with the exact code from which it was
generated. No harness-only candidate map or duplicate assembly implementation exists.

Cell applicability is partitioned from the LOCKED BF2 seed manifest's own
`sourceMap` request input — never from candidate map presence or any
production-produced metadata — and every one of the 36 cells is accounted for as
map-enabled or map-disabled with none unclassified.

For every map-enabled cell, the genuine production assembler returns code and a
map together; the code is byte-identical to the pinned pre-amendment baseline; the
emitted map passes independent wire validation; the PRODUCTION serialization is
deterministic across repeated identical invocations (independent of, and in
addition to, artifact-level equality — production's own `map_hash` is computed
over raw serialized bytes, so two valid but differently-encoded serializations of
the same logical artifact would defeat that hash's purpose even though they would
pass the decoded-artifact comparison); and the complete DECODED artifact equals the
independently implemented, input-only reference exactly — under the two authorized
sequential `CodeTransform` rewrites and the frozen specification's chaining
algebra — with the complete, schema-bound vector suite delivered, independently
reviewed, and reproduced exactly by both implementations before this exit is
claimed. For every map-disabled cell, no map is produced, and that absence is
asserted independently rather than by omitting the check.

Missing or uncomposable required input maps fail closed (see Abort/rescope). BF2's
authored-source oracle runs once per cell over the genuine production result
through its accepted entry point, unchanged, and the candidate records that it RAN
for every cell; only its non-clean MAPPING verdict is excluded from BV0A's gate.
Residual fragment-emitter violations are BV0's acceptance responsibility, not
BV0A's.

All pre-existing assembled-code bytes and successful parse, link, normalized
structure/helper-topology, deterministic runtime/server, diagnostic, and public-route
results remain unchanged. Applicable locked performance cells remain within their
existing thresholds. The change is Vue-only and adds no B3/B4/BV1/B5 authority, no
universal IR, no alternate production assembly route, and no waiver or deferral
artifact.

## Abort/rescope

Stop with `RESCOPE_REQUIRED` if a correct map for the exact bounded seed domain
requires B3's canonical request, B4's general logical-unit/identity/publication
architecture, BV1's complete Vue plan, B5's direct-core cutover, a universal or
cross-framework IR, a new public product contract, or any change to BF2's accepted
authored-source mapping oracle or its invocation. Oracle and invocation immutability
is unconditional; BV0A has no exception for a change its composition work appears to
require.

Stop with `RESCOPE_REQUIRED` if an input script or template map is
`UncomposableInputMap` — a structural input defect BV0A cannot faithfully carry
forward, causing a hard fail-closed (or rescope to the true owner), never coerced
into an empty, approximate, or unmapped successful result: malformed map JSON;
wrong/missing version; undecodable or out-of-range wire data; malformed table
rows; an indexed/non-flat map; a dangling table index; an out-of-fragment or
surrogate-split coordinate; incompatible cross-fragment table metadata. Rescope
oracle-side or invocation issues to BF2 and script-emitter or template-emitter
issues to BV0; never correct either opportunistically inside BV0A.

Two exclusions are explicit, because both are otherwise available as an escape:

- A template-only cell whose compiler produced a SYNTHETIC script block with an
  empty map is NOT a missing required map — it is synthetic sourceless code,
  composed as such. Map-requiredness comes from the pre-assembly authored-fragment
  inventory, never from `compiled.script.is_some()`.
- A mechanically composable but oracle-INVALID fragment mapping — one that decodes
  and composes cleanly while pointing somewhere the authored fixture does not
  justify, such as the diagnosed `const`-to-`<` segment — is NOT grounds for
  `RESCOPE_REQUIRED` and NOT a BV0A defect. It is carried forward faithfully and is
  a mandatory BV0 bug.

An exact-equality failure against the reference is BV0A's OWN composition defect by
definition, and is fixed in BV0A rather than rescoped. Do not invent source
identity, approximate an exact mapping, silently omit a required segment, broaden
into a fragment-emitter rewrite, or defer the literal BV0 exit while reporting
BV0A complete.
