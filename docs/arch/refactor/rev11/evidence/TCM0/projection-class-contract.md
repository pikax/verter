# TCM0 §5 — Projection-class contract

Scope: charter item 5. Ratify the minimal class set and the terminal policy deriving TypeScript feature
masks from class × relation × region × owner × certified capability. Every wire span gets an explicit
mask — never omitted into the upstream all-features default.

## The upstream mask primitive (verified, not invented)

The candidate package already carries a per-segment feature mask on the wire —
`package-lock-and-semantic-api.md` §3, `dist/ast/spanMap.d.ts`. Restated here because the class
contract below is defined entirely in terms of it:

```
SpanMapKind:    Verbatim=0, Atom=1, Alias=2                          (copy semantics)
SpanMapFidelity: Exact=0, Atom=1, Approximate=2, None=3              (mapping precision)
SpanMapFeature: 20-bit flags — Hover, SignatureHelp, Completion, Definition, TypeDefinition,
                Implementation, References, DocumentHighlights, Rename, CallHierarchy, CodeActions,
                Formatting, InlayHints, SemanticTokens, FoldingRanges, SelectionRanges, LinkedEditing,
                AutoInsert, DocumentSymbols, CodeLens, None=0, All=1048575
```

`SpanMapFeature.All` (`1048575`) is the upstream all-features default a segment gets when a content
mapper does not explicitly restrict it — this is exactly the state the charter forbids ("never omitted
into the upstream all-features default"). Every `SpanMapSegment` Verter's content mapper emits (TCM2)
must carry an explicit, computed `features` value; `undefined`/omitted is a defect, not a convenience
default, because `undefined` on the wire normalizes to `All` (confirmed:
`dist/ast/spanMap.d.ts` — `NormalizedSpanMapSegment` "omitted features have been normalized to `All`").

## Correction, 2026-08-23: a fifth class was missing — reconciled to the steering, not invented

The four classes below (`AuthoredVerbatim`, `AuthoredTransformed`, `SynthesizedHelper`, `ExternalUnit`)
are the set this file originally ratified. The maintainer's steering
(`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §8) is explicit and normative on a case this set
did not name: "A synthesized definition target is represented as: `relation = Atom`,
`original_len = 0`, `projection_class = DefinitionAnchor`." `DefinitionAnchor` is therefore a REQUIRED
fifth class, added below as class 5 — this is a reconciliation to already-ratified steering text, not a
new taxonomy invented at integration time. This does not reopen the RELATION model: the steering is
equally explicit that "There is no fourth `Anchor` relation" — the three generic relations
(`ExactCopy`/`Atom`/`IdentityAlias`, TypeScript-terminal `Verbatim`/`Atom`/`Alias`) are unchanged;
`DefinitionAnchor` is a `projection_class` value, not a fourth relation, and it always pairs with
`relation = Atom`.

## The minimal class set

Five classes, derived from the actual carrier-region taxonomy the External-Source Decision Table
(`external-source-decision-table.md`) and the diagnostic matrix (`diagnostic-ownership-matrix.md`)
already establish, plus the steering's explicit `DefinitionAnchor` requirement above — not a sixth
invented taxonomy:

1. **`AuthoredVerbatim`** — a generated span that is a byte-for-byte copy of authored source (e.g. an
   inline `<script>` block's own TS content, copied unchanged into the TSX). `SpanMapKind::Verbatim`,
   `SpanMapFidelity::Exact`. Mask: `All` (every feature is legitimately answerable and round-trips
   exactly) — this is the ONE class where the upstream all-features default happens to be the CORRECT
   explicit value, which must still be set explicitly (computed, not omitted) so a future narrowing of
   this class does not silently inherit `All` from an omission bug.
2. **`AuthoredTransformed`** — a generated span derived from authored source through a lossy-but-
   reversible transform (e.g. a directive's expression re-emitted inside a synthesized `if`/ternary; a
   template interpolation lifted into a hoisted variable). `SpanMapKind::Atom` or `Alias`,
   `SpanMapFidelity::Atom` or `Approximate`. Mask: the subset of `SpanMapFeature` that is
   position-stable across the transform — always includes `Hover`/`Definition`/`References`/
   `Completion`/`SignatureHelp`/`SemanticTokens`; excludes `Rename` when the transform is not
   text-identity-preserving (renaming the synthesized token would not correctly rewrite the authored
   source), excludes `CodeActions` when Verter cannot map an edit back losslessly.
3. **`SynthesizedHelper`** — a span with no authored counterpart at all (compiler-injected helper code,
   ambient declarations, framework prelude). Mask: `Hover | Definition` only when the helper is a stable,
   documented ambient symbol (e.g. `$props`) worth explaining; `None` otherwise. Never `Rename`,
   `CodeActions`, `References`, or `Completion` — there is no authored identity to rename, act on,
   reference, or complete against.
4. **`ExternalUnit`** — a span that logically belongs to a different source unit entirely (external
   `<script src>`/`<template src>`, a supplemental output). Mask: `All`, but the CARRIER is different —
   this class exists to make explicit that "external unit" is a *routing* distinction (which document
   owns the span), not a *feature-restriction* distinction; once routed to its own document, the span
   behaves as `AuthoredVerbatim` there.
5. **`DefinitionAnchor`** — a synthesized, zero-length definition target: `relation = Atom`,
   `original_len = 0` (steering §8, verbatim). This is NOT a `SynthesizedHelper` — a `SynthesizedHelper`
   span may have nonzero generated length and represents injected helper *code*; a `DefinitionAnchor` is
   specifically the zero-length target position a `go to definition` jump lands on (e.g. the anchor for a
   macro-synthesized binding that has no authored token of its own to point at). Mask: `Definition` only
   — never `Hover` (there is no authored content at a zero-length position to explain beyond what the
   definition jump itself conveys), never `Rename`/`CodeActions`/`References`/`Completion`/`SemanticTokens`
   (a zero-length position has no text to rename, act on, reference, complete against, or classify).
   Distinguish from `SpanMapKind::Atom` used for a nonzero-length `AuthoredTransformed` span: the
   `original_len = 0` discriminant is what selects `DefinitionAnchor` over `AuthoredTransformed`, not the
   `relation` alone (`Atom` relation appears in both).

## Terminal policy: class × relation × region × owner × certified capability → mask

The mask a wire span actually carries is the AND of:

- the class's baseline mask (above),
- the **relation** to its owning declaration (a heritage/inherited-attrs region masks out
  `Rename`/`CodeActions` even inside `AuthoredVerbatim`, since renaming a fallthrough-inherited attr
  must go through the component's own prop declaration, not the call site),
- the **region**'s framework ownership (a Svelte snippet region — currently an unsupported RUNTIME
  surface per `svelte/runtime/client_block_plan.rs:90-97`, the `BlockIr::Snippet` arm returning `UnsupportedSvelteRuntimeSurface::ComponentOrSnippet`, surfaced product-free by `svelte/carrier.rs:639-651` (corrected 2026-08-24: the previous cite `carrier.rs:1255-1259` is inside that file's `#[cfg(test)] mod tests`, opening at `:765` — the discriminating test, not the production refusal) but a supported IDE surface — masks out any feature the IDE
  path does not itself support, independent of the class computed above),
- the **owner** assigned in `feature-ownership-ledger.md` (a `VerterNative`-owned feature has no
  TypeScript-side mask entry at all — it never rides the wire's `SpanMapFeature`, it is answered
  entirely inside Verter's own LSP handler),
- the **certified capability** of the exact candidate package in use (`package-lock-and-semantic-api.md`
  §4e records that this candidate has NO cancellation primitive — so no mask may imply a
  cancellable-in-flight feature; a future candidate that adds cancellation would widen this policy, a
  regression would need to narrow it back, and the certification step is what gates which is true at
  any given package pin).

## What this contract forbids

- Any code path that omits `features` on a minted `SpanMapSegment` (normalizes to `All` — the exact
  defect this contract exists to prevent).
- A sixth class invented ad hoc by a later block instead of extending this table (extension is fine;
  silent proliferation is not — the ADR TCM1 produces must reference this file, not restate it).
- Pairing `projection_class = DefinitionAnchor` with a nonzero `original_len`, or pairing it with any
  `relation` other than `Atom` — the steering's exact tuple is `(relation = Atom, original_len = 0,
  projection_class = DefinitionAnchor)`, not a looser approximation.
- Deriving a mask from string/name heuristics on the span's content — masks are derived structurally
  from the class/relation/region/owner tuple above, consistent with the Typed-IR-Only Resolver Rule
  already governing the rest of Verter's semantic surface.

## Closure, 2026-08-23: the terminal policy as a TOTAL function (`G-PROJECTION-MASK-TOTALITY`)

`OPEN-GAPS.md`'s `G-PROJECTION-MASK-TOTALITY` row records that the "Terminal policy" section above names
five factors but does not compute a mask for every combination of them — `AuthoredTransformed`'s and
`SynthesizedHelper`'s baselines are written as prose conditionals ("excludes `Rename` when the transform is
not text-identity-preserving", "`None` otherwise"), leaving several of the 20 bits undecided. A contract
that reads as terminal but is not is the thing TCM2's terminal-mask emission cannot consume.

This section replaces the prose with a total function. Totality is achieved structurally: **every factor is
a total map from a CLOSED domain to a 20-bit constant, and every conditional is either relocated to the
factor that actually decides it or given a fail-closed default with a closed widening set.** No axis
combination is left to judgement at emission time.

### The exact bit values, read from the shipped enum

Restated from `dist/enums/spanMapFeature.js` in the pinned candidate (generated from
`tsc/internal/spanmap/spanmap.go`) so every constant below can be recomputed:

```
Hover=1  SignatureHelp=2  Completion=4  Definition=8  TypeDefinition=16  Implementation=32
References=64  DocumentHighlights=128  Rename=256  CallHierarchy=512  CodeActions=1024
Formatting=2048  InlayHints=4096  SemanticTokens=8192  FoldingRanges=16384  SelectionRanges=32768
LinkedEditing=65536  AutoInsert=131072  DocumentSymbols=262144  CodeLens=524288
None=0  All=1048575
```

### The function

```
mask(class, relation, region, owner_policy, capability_pin)
    = CLASS_BASELINE[class]
    & RELATION[relation]
    & REGION[region]
    & OWNER_WIRE_ELIGIBLE
    & CAPABILITY[capability_pin]
```

Each factor is defined below over its whole domain. `supportsFeature` upstream is
`(segment.features & feature) !== 0` (`dist/ast/spanMap.js:354-356`), so an AND of masks is exactly the
composition upstream evaluates — the five factors are independently restrictive, which is what makes the
AND well-defined rather than an ordering question.

#### Factor 1 — `CLASS_BASELINE`, five classes, unconditional constants

| Class | Baseline | Value | Note |
|---|---|---|---|
| `AuthoredVerbatim` | `All` | 1048575 | unchanged |
| `AuthoredTransformed` | `All` | 1048575 | **changed from prose.** Its two conditional exclusions were both conditions on the transform's reversibility, which is exactly what `relation` encodes — they are relocated to factor 2, leaving this baseline unconditional. |
| `SynthesizedHelper` | `None` | 0 | **changed from prose.** The original "`Hover \| Definition` only when the helper is a stable documented ambient symbol; `None` otherwise" is resolved fail-closed: the baseline is `None`, and a helper span is widened to `Hover \| Definition` (9) only if its symbol is a member of the closed `DocumentedAmbientSymbol` registry defined below. |
| `ExternalUnit` | `All` | 1048575 | unchanged — a routing distinction, not a feature restriction |
| `DefinitionAnchor` | `Definition` | 8 | unchanged |

**The `DocumentedAmbientSymbol` registry.** A closed, enumerated set of synthesized ambient symbols that
are stable, documented, and worth explaining to a user. Membership is by exact symbol identity, never by
name pattern (consistent with this contract's own ban on deriving masks from string heuristics). Its
initial member is `$props`. Adding a member is an amendment to this contract and a reviewable diff; a
helper span whose symbol is not a member emits `SpanMapFeature.None`, which is a legal explicit value and
not an omission. This keeps the factor total — every synthesized span gets 0 or 9, decided by set
membership — while preserving the fail-closed direction the charter requires.

#### Factor 2 — `RELATION`, three relations, derived from upstream's own fidelity rule

The governing rule is already RATIFIED, and this contract should have cited it directly rather than
deriving an equivalent from the package: the steering states
*"edit-producing operations require exact length-preserving `ExactCopy`"*
(`rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md:525`). That single line settles the factor. The
package-level derivation below is retained as independent corroboration, and because it supplies the
mechanism the steering line asserts:

- `SpanMap.isExact` is documented as *"a precise, **edit-safe** projection through one verbatim segment"*
  (`dist/ast/spanMap.d.ts`). Upstream itself ties edit safety to `SpanMapFidelity::Exact`.
- Only a `Verbatim` segment produces `Exact`. `mapPoint` (`dist/ast/spanMap.js:159-164`) and `mapRange`
  (`:138-143`) both read `segment.kind === SpanMapKind.Verbatim ? …Exact : …Atom`. **`Alias` and `Atom` are
  not distinguished anywhere in the mapping code** — both yield `Atom` fidelity.

So the edit-producing bits — `Rename | CodeActions | Formatting | LinkedEditing | AutoInsert` = 199936 —
are legal only on an `ExactCopy` relation, and the read-only complement is `All & ~199936` = **848639**.

| Relation | Terminal `SpanMapKind` | Mask | Value |
|---|---|---|---|
| `ExactCopy` | `Verbatim` | `All` | 1048575 |
| `Atom` | `Atom` | read-only complement | 848639 |
| `IdentityAlias` | `Alias` | read-only complement | 848639 |

`IdentityAlias` equalling `Atom` is not a conservative guess — it is what upstream's mapping does, cited
above. If a future candidate distinguishes them, this row widens, and the capability factor is where that
change is gated.

**The `ExactCopy` row is length-conditional, not unconditional** (added 2026-08-23). `RELATION[ExactCopy]`
is `All` only for a mapping that stays inside ONE verbatim segment. `mapRange`
(`dist/ast/spanMap.js:150`) returns `Approximate` fidelity for a range that starts in one segment and ends
in another, even when both are `Verbatim` — so a naive reading of this row would be fail-OPEN for a
cross-segment edit. The steering's wording forecloses it: the requirement is *exact **length-preserving**
`ExactCopy`*, and a range spanning two segments is not length-preserving with respect to either. The
emitter obligation is therefore: **an edit-producing bit may be set on an `ExactCopy` span only when the
span lies wholly within a single verbatim segment.** In practice factor 4 leaves only `CodeActions` among
the edit bits, so this constrains exactly one bit today — but it is stated because the constraint is a
property of the relation, not of which bits happen to survive the owner factor.

#### Factor 3 — `REGION`, total by construction via a mandatory default

`region` is the one axis whose domain is not closed today, because regions arrive with carriers TCM1-TCM3
have not yet produced. It is made total the only way an open domain can be: a **mandatory default plus a
closed exception table**.

```
REGION[r] = EXCEPTIONS[r] if r ∈ EXCEPTIONS else All      // All = 1048575
```

**`EXCEPTIONS` is empty today, and that is a finding rather than an omission.** Every content-mapped region
in `external-source-decision-table.md` is a fully IDE-supported surface — including the Svelte snippet
region, which row 5 records as *"content-mapped for the IDE-checked halves"* even though its runtime half is
unsupported (`svelte/runtime/client_block_plan.rs:90-97` via `svelte/carrier.rs:639-651`; the test formerly cited here, `carrier.rs:1255-1259`, sits inside `#[cfg(test)]`). Runtime-unsupported is a codegen property, not an IDE-feature
property, so it justifies no mask restriction. TCM0 therefore adds no exception row rather than inventing
one to make the factor look substantive.

The obligation this places on TCM2 is explicit: a new region either matches an exception row or takes `All`
— it may never be given an ad-hoc mask at the emission site. Adding an exception row is an amendment to
this contract.

#### Factor 4 — `OWNER_WIRE_ELIGIBLE`, one constant derived from the ownership ledger

This factor is per-FEATURE, not per-span: a `SpanMapFeature` bit may ride the wire at all only if
TypeScript is a legitimate answerer for that feature. A feature owned exclusively by `VerterNative`,
`VerterWithTypeSemanticOracle` or `DisabledByExplicitApprovedContract` is answered entirely inside Verter's
own LSP handler and must never be set on a mapper segment — setting it would invite TypeScript to answer a
question Verter owns.

Computed from `feature-ownership-ledger.md`'s owner column and its capability-coverage closure:

| Bit | Ledger row | Owner | Eligible? |
|---|---|---|---|
| `Hover` | #9 | `TypeScriptLspDirect` (plain TS) + oracle | **yes** |
| `SignatureHelp` | #16 | `TypeScriptLspDirect` | **yes** |
| `Completion` | #6 | `TypeScriptLspDirect` (plain TS) + oracle | **yes** |
| `Definition` | #12 | `TypeScriptLspDirect` + oracle | **yes** |
| `TypeDefinition` | #13 | as #12 | **yes** |
| `References` | #14 | as #12 | **yes** |
| `DocumentHighlights` | #19 | as #12 | **yes** |
| `CodeActions` | #17 | `TypeScriptLspDirect` (plain-TS fixes) + `VerterNative` | **yes** |
| `InlayHints` | #20 | `TypeScriptLspDirect` | **yes** |
| `SemanticTokens` | #18 | `TypeScriptLspDirect` (script) + `VerterNative` (template) | **yes** |
| `Rename` | #15 | `VerterWithTypeSemanticOracle` **only** — the row states a pure `TypeScriptLspDirect` answer *"would miss the template-side occurrences"* | no |
| `Implementation` | — | **not a `TypeProvider` capability** — no trait method, no `verter_lsp` handler, no advertised `implementation_provider`; served by the typescript-plugin carrier-routing override at `packages/typescript-plugin/src/index.ts:3095` (ledger closure §3; corrected 2026-08-24 — previously recited here as "does not exist in this codebase") | no |
| `CallHierarchy` | — | `VerterNative` (`crates/verter_lsp/src/features/call_hierarchy.rs:15`) | no |
| `Formatting` | — | `VerterNative` (`crates/verter_lsp/src/features/formatting.rs:23`) | no |
| `FoldingRanges` | — | `VerterNative` (`crates/verter_lsp/src/features/folding_range.rs:14`) | no |
| `SelectionRanges` | — | `VerterNative` (`crates/verter_lsp/src/server/aux_features.rs:112`) | no |
| `LinkedEditing` | — | `VerterNative` (`crates/verter_lsp/src/server/aux_features.rs:1039`) | no |
| `DocumentSymbols` | — | `VerterNative` (`crates/verter_lsp/src/features/document_symbol.rs:16`) | no |
| `CodeLens` | — | `VerterNative` (`crates/verter_lsp/src/features/code_lens.rs:13`) | no |
| `AutoInsert` | — | `VerterNative`. **CORRECTED 2026-08-23:** this cell previously read "no ledger row and no LSP handler — fail-closed", and the "no LSP handler" half is false: Verter handles `textDocument/onTypeFormatting` at `crates/verter_lsp/src/server/aux_features.rs:1158`, dispatched at `crates/verter_lsp/src/server/mod.rs:1603`, advertised at `crates/verter_lsp/src/capabilities.rs:109-112` (trigger `>`), auto-inserting a closing tag via `crates/verter_lsp/src/features/auto_close_tag.rs`. It is ineligible because it is Verter-owned, exactly like the other nine — not because it is absent. The derived constant is unchanged | no |

```
OWNER_WIRE_ELIGIBLE = Hover|SignatureHelp|Completion|Definition|TypeDefinition
                    |References|DocumentHighlights|CodeActions|InlayHints|SemanticTokens
                    = 13535
```

This factor is the single largest simplification in the policy, and it resolves the prose's hardest case on
its own: **`Rename` is cleared globally**, because the ledger already assigns rename to the oracle for a
reason the mask cannot override — a template-spanning rename is not answerable from the mapped file.

**Reconciliation with `feature-ownership-ledger.md` row #15, which said the opposite** (added 2026-08-23).
Row #15's "Required TS capability" cell reads `SpanMapFeature.Rename` and its mapping cell reads
"mask `Rename`", which a TCM2 implementer would read as an instruction to SET that bit. That is
incompatible with this factor, and this factor is the correct side of the contradiction: row #15's own
owner column assigns rename to `VerterWithTypeSemanticOracle` precisely because TypeScript cannot see the
template-side occurrences, so setting `SpanMapFeature.Rename` on a mapper segment would invite TypeScript
to answer a rename Verter owns and produce a partial edit — the exact failure the Project-Bound
External-TS Contract's rename fail-closed rule exists to prevent. Row #15's capability/mask cells are
therefore superseded by this factor: **the correct wire value is `Rename` CLEARED**, and the ledger cell is
a stale artifact of the pre-mask-contract draft. Row #15 is the only mask-bearing row with this conflict;
the other ten were checked. The
`AuthoredTransformed` prose agonised over when to exclude `Rename`; the answer is always.

Note this factor makes the *edit*-bit logic in factor 2 nearly vacuous in practice: of the five
edit-producing bits, only `CodeActions` survives owner eligibility. Factor 2 is retained in full anyway,
because it is the factor that stays correct if an owner assignment later changes.

#### Factor 5 — `CAPABILITY[capability_pin]`, gated on the certified package

```
CAPABILITY["typescript@7.1.0-dev.20260822.1"] = All = 1048575
```

None of the 20 bits names a cancellation primitive, so §4e's cancellation absence
(`package-lock-and-semantic-api.md`) constrains TCM3's implementation, not this mask. The same is true of
the three probe findings in §6.2: the absence of a project-wide references primitive, the outright
rejection of completion lists needing auto-imports, and the position-degrade behaviours all constrain
**how** a feature is served, not **whether** a segment may advertise it. Recorded explicitly so a reviewer
can check the reasoning rather than take the `All` on trust.

A later candidate narrows this factor if it removes a capability, and widens it if it adds one — and
re-certification is what decides which. That is the whole purpose of this factor being separate.

### The resulting table, computed

Fifteen cells, one per `class × relation`, after ANDing the empty-exception `REGION` and the full
`CAPABILITY`:

| Class | `ExactCopy` | `Atom` | `IdentityAlias` |
|---|---|---|---|
| `AuthoredVerbatim` | **13535** | 12511 | 12511 |
| `AuthoredTransformed` | 13535 | **12511** | **12511** |
| `SynthesizedHelper` | 0 | 0 | 0 |
| `SynthesizedHelper` ∈ registry | 9 | **9** | 9 |
| `ExternalUnit` | **13535** | 12511 | 12511 |
| `DefinitionAnchor` | 8 | **8** | 8 |

Bold marks the cells reachable given each class's own relation constraint (`AuthoredVerbatim` and
`ExternalUnit` are `Verbatim` by definition; `AuthoredTransformed` is `Atom` or `Alias`; `DefinitionAnchor`
always pairs with `Atom` per steering §8). The unreachable cells are still computed, because a total
function has no holes — an emitter that reaches one has a class-assignment bug, not a mask bug, and it will
emit a defensible value while that bug is found.

Decoded:

- **13535** = `Hover|SignatureHelp|Completion|Definition|TypeDefinition|References|DocumentHighlights|CodeActions|InlayHints|SemanticTokens`
- **12511** = the same, minus `CodeActions`
- **9** = `Hover|Definition`
- **8** = `Definition`

Recomputable in one line:

```js
const F={Hover:1,SignatureHelp:2,Completion:4,Definition:8,TypeDefinition:16,Implementation:32,
References:64,DocumentHighlights:128,Rename:256,CallHierarchy:512,CodeActions:1024,Formatting:2048,
InlayHints:4096,SemanticTokens:8192,FoldingRanges:16384,SelectionRanges:32768,LinkedEditing:65536,
AutoInsert:131072,DocumentSymbols:262144,CodeLens:524288}, ALL=1048575;
const EDIT=F.Rename|F.CodeActions|F.Formatting|F.LinkedEditing|F.AutoInsert;   // 199936
const READ=ALL&~EDIT;                                                          // 848639
const OWNER=F.Hover|F.SignatureHelp|F.Completion|F.Definition|F.TypeDefinition
           |F.References|F.DocumentHighlights|F.CodeActions|F.InlayHints|F.SemanticTokens; // 13535
```

### What this does and does not close

**What this section produced.** The policy is written as a total function: five factors, each total over
its domain, composing by AND into an explicit value for every combination. TCM2's terminal-mask emission
can consume it without making a policy decision of its own, which is what `G-PROJECTION-MASK-TOTALITY`
asked for. The `undefined`-becomes-`All` defect the contract exists to prevent is not reachable by
accident through it, because every path through the function yields a computed constant. Each factor's
derivation is cited to the shipped source or to the ownership ledger, and those citations stand as
evidence.

**The "therefore CLOSED" verdict is WITHDRAWN.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q1 returns this block's round-3 candidate as wrongly scoped, lands its work as a NON-ACCEPTANCE evidence
package, and hands the incomplete contract remainder to a successor block **with fresh verification** — and
totality is exactly the kind of claim that must be independently checkable rather than self-certified: it
is a statement about all fifteen `class × relation` cells and all twenty feature bits, provable only by
someone re-deriving the table. `G-PROJECTION-MASK-TOTALITY` is therefore OPEN with the successor as owner
(`OPEN-GAPS.md`; scope: `successor-block-scope.md`).

**Not closed, and unchanged by this section.** `feature-ownership-ledger.md`'s reconciliation note defers
per-row `projection_class` ASSIGNMENT for the `TokenCompletion` grouping to TCM1/TCM2. That is a different
question — which class a given span belongs to — and this section makes no claim about it. The mask
function is total over the class axis; choosing a span's class remains TCM1/TCM2's named task.

**Two obligations this places on TCM2**, both of which are amendments to this contract rather than local
decisions: adding a `REGION` exception row, and adding a `DocumentedAmbientSymbol` registry member.
