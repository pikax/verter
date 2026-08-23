# TCM0 — The string-encoded projection surface (feeds TCM1 directly)

This file exists because the amendment names this migration as the ASAP/load-bearing element ("That
migration — TCM1 — is the load-bearing element; the rest sequences behind it") and TCM0's charter demands
recording more than the two originally-cited lines. **This file records a best-effort, substantially
wider inventory than the original two-line citation — it does NOT claim to be the exhaustive full
extent** (see the 2026-08-23 correction section below: two independent manual/mechanical passes each
undercounted this surface, so a third specific-number claim is deliberately not made). Full detail
(call-site-by-call-site, as far as this investigation traced it) lives in the sub-investigation transcript
this file summarizes; this is the load-bearing STARTING POINT TCM1 must build from, not a closed
migration universe.

## Correction to the amendment/discovery text

Both the amendment and discovery documents state `checker.rs:411` calls
`PositionMapper::from_json(… .unwrap_or(""))`. **Verified false as written**: `checker.rs:411` base64-
encodes the raw JSON string directly into an inline `//# sourceMappingURL=data:application/json;base64,…`
comment for `tsc`/`tsgo` to parse independently — it never calls `PositionMapper::from_json` at all
(`grep -n "PositionMapper" crates/verter_tsc/src/checker.rs` returns no hits). The
`PositionMapper::from_json` call the two documents cite is real, but lives only in the TEST
`crates/verter_lsp/tests/cases/kebab_tag_mapping_full_columns.rs:65`, exercising a similarly-shaped
compile independently of `checker.rs`. This is a narrow but real inaccuracy in the ratified amendment
text, recorded here rather than silently corrected in place — the amendment's core thesis (the surface is
string-encoded and must become typed) is unaffected; only the specific citation is corrected.

## What `source_projection_map()` actually is

Not a free function — a method, with two producer-side struct fields, all `Option<String>`/`Option<&str>`:
`AssembledArtifact::source_projection_map` (`crates/verter_compiler/src/assembly/publish.rs:75-77`),
backed by `ArtifactContribution.source_projection_map: Option<String>` (`publish.rs:38`), fed by
`VerterTsxBlock.source_map: String` (`compile/types.rs:495`, doc: "JSON source map string"). **Confirmed:
a JSON string today, never a struct that's later serialized.**

Producer chain (assembly boundary):
```
CodeTransform::generate_map_json_with_preamble()   compile/mod.rs:2102        → String
  → VerterTsxBlock.source_map                       compile/types.rs:495
    → standalone.rs:272  ArtifactContribution.source_projection_map = Some(...)
      → assembly::publish()                          assembly/publish.rs:207-217,277
        → AssembledArtifact.source_projection_map()   assembly/publish.rs:75-77
```

`PositionMapper::from_json` (`crates/verter_lsp/src/documents/position_map.rs:200-215`) expects a
standard V3 source-map JSON string PLUS one Verter-owned extension member
(`x_verter_helper_preamble_end: {line, character}`) that `oxc_sourcemap` silently drops and that
`PositionMapper` recovers via a SECOND, independent `serde_json` parse of the same string
(`PreambleEnvelope` struct at `position_map.rs:142`, consumed at the `serde_json::from_str::<PreambleEnvelope>(json)` call inside `from_json` at `position_map.rs:206`) — a double-parse cost that exists **only** because the
transport is a bare string with no typed side-channel. This doubled parse is itself part of the cost
TCM1 removes.

Production consumers of `PositionMapper` (8 non-test call sites, all `crates/verter_lsp/src/`):
`documents/mod.rs:652,1068,1161,1240,1317`; `provider_surface_store/producers.rs:684,957`;
`server/rename_plan.rs:518`. All eight `and_then`/`.ok()` past a failed parse rather than hard-erroring —
a malformed string degrades a feature silently today; a typed product removes the parse-failure class
entirely (a type that parses successfully by construction rather than being validated after the fact).

## The four named products do not exist as data-carrying types today

Central finding: `PlacementMap`, `SourceProjectionMap`, `RuntimeSourceMapData`, `EncodedSourceMap` exist
ONLY as zero-payload SHA-256 identity newtypes (`crates/verter_identity/src/mapping.rs:6-25`,
`digest_identity!` macro) — cache-key wrappers with no map data field at all. The actual DATA for all four
is uniformly `String`/`Option<String>`/`Option<Arc<str>>`/`&str`, scattered as differently-named fields.

**Correction, 2026-08-23 (round-2 review, then re-verification, then a second independent architecture
review of THAT re-verification).** The original count here ("at least nine... plus four", restated by
`TCM1.md` as "the thirteen fields") was wrong: round 2 found two of the eleven cited struct names —
`SvelteClientOutput` and `SvelteIdeProjector` — do not exist anywhere in the tree, and `CssProcessResult`
likewise names no real struct.

A first re-derivation attempt (`grep -rn "pub source_map" --include="*.rs"
crates/verter_compiler/src/`, each hit resolved to the nearest preceding `pub struct`/`struct`
declaration) found 24 fields and was committed as a claimed CLOSED count. **A follow-up architecture
review proved that claim wrong too** — the exact failure mode the round-2 report had already named as
the risk of re-deriving this inventory under review pressure. Two concrete errors in that attempt:

1. **A wrong struct attribution survived the re-derivation.** `vue_module.rs:383`'s `source_map` field
   belongs to `ComposedFragments` (`assembly/vue_module.rs:380-385`), not `MainFragmentTag<'a>` — the
   struct-resolution heuristic (nearest preceding `struct` keyword) picked the wrong one because another
   struct is declared between the two.
2. **The `pub source_map`-literal grep pattern is too narrow to find the surface** — it misses: enum
   VARIANT fields, which are unprefixed even when the enum itself is `pub` (`StyleRewriteOutcome::
   Rewritten.source_map: String`, `style_planner.rs:171`); differently-NAMED fields that carry the same
   map-data convention (`VueMainModuleRequest.template_map_json`, `vue_module.rs:220`;
   `ArtifactContribution.source_projection_map`/`.runtime_source_map`, `publish.rs:38-39`, both already
   named elsewhere in THIS SAME FILE's "producer chain" section above but never merged into the central
   count; `PendingRuntime.runtime_source_map`, `standalone.rs:435`, a local fn-scoped struct); and PRIVATE
   fields accessed only through accessor methods (`AssembledArtifact`'s own `source_projection_map`/
   `runtime_source_map` fields, `publish.rs:52-53`, private — the grep for `pub source_map` structurally
   cannot see a non-`pub` field regardless of struct visibility; and `ScopedCssRender.source_map:
   Option<String>` at `svelte/runtime/css/render.rs:75`, a `pub(crate)` field the same grep also missed
   because the field itself is not literally `pub`).

**This file does NOT attempt a third closure claim.** Two independent manual/mechanical passes have now
undercounted this surface (round-2's citation check found name errors; this pass's grep-based
re-derivation found a wrong attribution plus at least 8 more fields by three additional naming/visibility
patterns the grep pattern structurally cannot reach). A textual/grep method cannot safely prove
EXHAUSTIVENESS for a "does this field carry map-data" question, because the answer depends on semantic
judgment (a doc comment, a call-site read) that a single regex cannot generalize across enum variants,
alternate names, and visibility levels at once. **Closing this count exactly requires either a
type-aware tool as a one-time migration-planning aid (a `syn`-based scan or equivalent, run to SEQUENCE
the work, not landed as an ongoing guard — a landed name/text scanner is itself forbidden by this
program's structural-enforcement rule), or accepting that TCM1's own migration work — not a pre-computed
count in this file — is the actual completeness authority.** TCM1's exit criterion 1 states the real,
COMPILER-ENFORCED proof method — SCOPED, not absolute: `CodeTransform`'s `generate_map_json*`
string-returning producer methods are DELETED (not kept alongside a typed variant), so every DIRECT caller
that received a string from them fails to compile until migrated — completeness for THAT chain comes from
the Rust type checker rejecting every site that still expects a string, not from a text search finding
every site by name. This does NOT, on its own, cover three named exceptions the same criterion states
explicitly: retained read-time JSON projection methods (a distinct, sealed, reviewed set, not closed by
deletion), the FFI/NAPI/WASM terminal wire serialization (criterion 3's own proof), and externally-supplied
map fields with no `CodeTransform` producer relationship at all (out of TCM1's scope, e.g.
`FfiBlockOverrideEntry`'s field, fed from caller input, not from `CodeTransform`'s output — corrects this
file's own earlier citation of that field as `FfiPreprocessResult`, which names a different type).
TCM1's own migration work — not a pre-computed count in this file — is the actual completeness authority
for what deletion does not already prove. This file's role is a best-effort STARTING inventory for
planning purposes, not a
closed migration universe and not itself the completeness proof.

The fields confirmed real as of this correction (name-corrected from the original three wrong citations,
plus the one wrong attribution and 8 omissions this pass found — **not asserted to be the complete set**),
by struct (file:line; all `String` unless noted):

`VerterScriptBlock.source_map` (`compile/types.rs:411`), `VerterTemplateBlock.source_map`
(`compile/types.rs:428`), `GeneratedCodeChunk.source_map` (`compile/types.rs:488`),
`VerterTsxBlock.source_map` (`compile/types.rs:495`), `VueStyleCascadeOutcome.source_map`
(`style_planner.rs:985`), `SvelteIdeProjection.source_map` (`svelte/ide/projector/mod.rs:99` — the real
name of the struct the original inventory called `SvelteIdeProjector`), `ClientModule.source_map:
Option<String>` (`svelte/runtime/client_output.rs:13` — the real name of the struct the original
inventory called `SvelteClientOutput`), `ScopedCssArtifact.source_map: Option<String>`
(`svelte/runtime/client_output.rs:52` — a SECOND field in the same file the original inventory missed
entirely), `ProvenStyleScopePlan.source_map: Option<String>` (`svelte/runtime/css/types.rs:374`),
`ComposedOutput.source_map` (`assembly/compose.rs:19` — the field the original inventory named generically
as "`assembly/compose.rs` fragment"), `SequencedOutput.source_map` (`assembly/compose.rs:225` — a SECOND
field in the same file the original inventory missed), `ProcessStyleResult<'a>.source_map:
Option<String>` (`css/types.rs:81` — the real name of the struct the original inventory called
`CssProcessResult`), `Fragment.source_map: Option<String>` (`assembly/fragment.rs:227`),
`ComposedFragments.source_map` (`assembly/vue_module.rs:383` — CORRECTED attribution; the prior pass
wrongly named `MainFragmentTag<'a>`, which has no such field), `IdeOutput.source_map`
(`framework_common/carrier_compiler.rs:56`), `RuntimeBlockContentInput.source_map: Option<Arc<str>>`
(`framework_common/carrier_compiler.rs:477`), `RuntimeMainModule.source_map`
(`framework_common/carrier_compiler.rs:572`), `RuntimeScriptBlock.source_map`
(`framework_common/carrier_compiler.rs:583`), `RuntimeTemplateBlock.source_map`
(`framework_common/carrier_compiler.rs:617`), `RuntimeStyleBlock.source_map: Option<String>`
(`framework_common/carrier_compiler.rs:639`), `TscOutput.source_map` (`tsc/script.rs:344`),
`GeneratedChunkOutput.source_map` (`framework_common/generated_chunk.rs:10` — the real name of the struct
the original inventory called `GeneratedChunk`), `GeneratedUnit<'a>.source_map: &'a str`
(`framework_common/generated_chunk.rs:15` — a borrowed second field in the same file), and
`QualifiedOutputSourceMap.raw_map: Option<String>` (`framework_common/carrier_compiler.rs:124`, unchanged
from the original inventory) — 24 by this pattern.

**Additionally found by the follow-up review, outside the `pub source_map`-literal grep pattern (not
exhaustive — see the hedge above):** `StyleRewriteOutcome::Rewritten.source_map: String`
(`style_planner.rs:171`, an enum variant field), `ScopedCssRender.source_map: Option<String>`
(`svelte/runtime/css/render.rs:75`, `pub(crate)`), `VueMainModuleRequest.template_map_json`
(`assembly/vue_module.rs:220`, differently named), `ArtifactContribution.source_projection_map:
Option<String>` and `.runtime_source_map: Option<String>` (`assembly/publish.rs:38-39`, differently named,
`pub`), `AssembledArtifact`'s own private `source_projection_map`/`runtime_source_map` fields
(`assembly/publish.rs:52-53`, private, exposed only via the `source_projection_map()`/
`runtime_source_map()` accessor methods at `publish.rs:75-80`), and `PendingRuntime.runtime_source_map:
Option<String>` (`standalone.rs:435`, a local fn-scoped struct) — 8 more, all confirmed present by direct
source read, none claimed to be the last ones remaining.

Excluded from the count (verified, not map data): three `bool`-typed "should a map be produced at all"
request flags (`compile/types.rs:292`, `framework_common/carrier_compiler.rs:342`,
`compile/mod.rs:2320`), and `RuntimeOutputDescriptor.source_map: QualifiedOutputSourceMap`
(`framework_common/carrier_compiler.rs:133`) — this one field already holds the TYPED wrapper, not a raw
string; it is the shape the others move toward, not another member of the string-encoded surface.

The `verter_protocol` NAPI/WASM wire types carry genuine `Option<String>` map-data fields. Three are
`CodeTransform`-produced and IN SCOPE for TCM1: `FfiVirtualFileResponse.source_map` (`:341`),
`FfiIdeResponse.source_map` (`:387`), and `FfiTscResponse.source_map` (`:397`). **Correction, round-6
review:** the fourth field this file previously attributed to `FfiPreprocessResult.source_map` at
`verter_protocol/src/types.rs:142` actually belongs to `FfiBlockOverrideEntry` (no `FfiPreprocessResult`
struct exists at that line) — and its own doc comment reads "Source map from the preprocessor, if
available," confirming it is CALLER-SUPPLIED inbound override data, not a `CodeTransform` output. Per
TCM1's exit criterion 1 correction (round 6), this field is explicitly OUT OF TCM1's scope — the
"four genuine map-data fields" count in the original correction pass above is now THREE in-scope fields
plus this one distinct, separately-owned, OUT-OF-SCOPE caller-supplied field (paired with a separate
`source_map_hash: Option<String>` at line 143 — a hash-of-the-map field, not map data itself, also
out of scope). (`types.rs:99` is a DIFFERENT, unrelated field — `FfiCompileOptions.source_map:
Option<bool>`, a request-side flag, not map data, and is not part of this count.)

**Best-effort count as of this correction: at least 32 verter_compiler fields (24 + 8) + 3 in-scope
verter_protocol fields = at least 35 IN-SCOPE, NOT claimed closed** (plus the one named out-of-scope
caller-supplied field above, tracked separately, not counted toward TCM1's migration universe). This is
materially more accurate than "at least nine"/"thirteen" and corrects a name AND an attribution error the
immediately-prior version of this file still
had, but it is explicitly NOT asserted to be the final, exhaustive count — see the hedge above. TCM1's
charter should cite "at least 36, not a closed universe — see this file's own hedge" rather than repeating
a specific total as if it were exact.

`CodeTransform` (`crates/verter_compiler/src/code_transform/code_transform.rs:48`) holds **no field of
any of the four product types** — only `chunks: Vec<Chunk<'a>>` (the geometry authority a typed
`SourceProjectionMap` would be derived from). The only typed intermediate anywhere in this surface is
`oxc_sourcemap::SourceMap<'static>` (an EXTERNAL crate type, not Verter-owned) returned transiently by
`CodeTransform::generate_map()` — and every production caller found (`compile/mod.rs:2102`,
`style_planner.rs:308-310`, `svelte/runtime/output.rs:204`, `svelte/runtime/css/render.rs:171`,
`svelte/ide/projector/mod.rs:317`) discards it to a string within the same call expression.

## What TCM1 must do (restated as an acceptance bar for that block, not executed here)

1. Introduce ONE Verter-owned typed `SourceProjectionMap` recorded inside `CodeTransform`, replacing the
   discard-to-string pattern at its single point of origin (`generate_map`/`generate_map_json*`,
   `code_transform/source_map.rs`) — not at each of the nine-plus downstream consumer sites individually.
2. Keep `PlacementMap`/`RuntimeSourceMapData`/`EncodedSourceMap` distinct per the amendment's explicit
   "may share packed primitives; not collapsed into a universal map" rule — TCM0 finds no evidence today
   that would justify collapsing them, and confirms they are genuinely different data (placement/chunk
   geometry vs. runtime map vs. terminal encoded bytes), just not yet typed as such.
3. Retire the double-parse `PreambleEnvelope` recovery once the preamble boundary is a typed field on the
   new product rather than a side-channel JSON member.
4. Extend the `verter_protocol` wire types onto the same typed product rather than leaving the FFI
   boundary on the old string convention while the in-process boundary moves — otherwise TCM1 creates
   exactly the "second string-encoded path left behind" outcome the Build Philosophy's "one clean
   cutover, not a merged dual-path transition" rule forbids.
