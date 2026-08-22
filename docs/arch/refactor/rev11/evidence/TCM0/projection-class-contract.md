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

## The minimal class set

Four classes, derived from the actual carrier-region taxonomy the External-Source Decision Table
(`external-source-decision-table.md`) and the diagnostic matrix (`diagnostic-ownership-matrix.md`)
already establish — not a fifth invented taxonomy:

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

## Terminal policy: class × relation × region × owner × certified capability → mask

The mask a wire span actually carries is the AND of:

- the class's baseline mask (above),
- the **relation** to its owning declaration (a heritage/inherited-attrs region masks out
  `Rename`/`CodeActions` even inside `AuthoredVerbatim`, since renaming a fallthrough-inherited attr
  must go through the component's own prop declaration, not the call site),
- the **region**'s framework ownership (a Svelte snippet region — currently an unsupported RUNTIME
  surface per `svelte/carrier.rs:1255-1259` but a supported IDE surface — masks out any feature the IDE
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
- A fifth class invented ad hoc by a later block instead of extending this table (extension is fine;
  silent proliferation is not — the ADR TCM1 produces must reference this file, not restate it).
- Deriving a mask from string/name heuristics on the span's content — masks are derived structurally
  from the class/relation/region/owner tuple above, consistent with the Typed-IR-Only Resolver Rule
  already governing the rest of Verter's semantic surface.
