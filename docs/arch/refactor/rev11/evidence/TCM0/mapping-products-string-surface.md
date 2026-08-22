# TCM0 — The string-encoded projection surface (feeds TCM1 directly)

This file exists because the amendment names this migration as the ASAP/load-bearing element ("That
migration — TCM1 — is the load-bearing element; the rest sequences behind it") and TCM0's charter demands
the FULL extent be recorded, not just the two cited lines. Full detail (call-site-by-call-site) lives in
the sub-investigation transcript this file summarizes; this is the load-bearing conclusion TCM1 must
start from.

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
`VerterTsxBlock.source_map: String` (`compile/types.rs:497`, doc: "JSON source map string"). **Confirmed:
a JSON string today, never a struct that's later serialized.**

Producer chain (assembly boundary):
```
CodeTransform::generate_map_json_with_preamble()   compile/mod.rs:2102        → String
  → VerterTsxBlock.source_map                       compile/types.rs:497
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
is uniformly `String`/`Option<String>`/`Arc<str>`, scattered as differently-named fields across at least
nine distinct struct definitions in `verter_compiler` (`VerterScriptBlock.source_map`,
`VerterTemplateBlock.source_map`, `GeneratedCodeChunk.source_map`, `VerterTsxBlock.source_map`,
`VueStyleCascadeOutcome.source_map`, `assembly/compose.rs` fragment `.source_map`,
`SvelteClientOutput.source_map`, `SvelteIdeProjector.source_map`, `CssProcessResult.source_map`,
`QualifiedOutputSourceMap.raw_map`, `GeneratedChunk.source_map`) plus genuine `Option<String>` map-data
fields on the `verter_protocol` NAPI/WASM wire types: `FfiPreprocessResult.source_map` at
`verter_protocol/src/types.rs:142` (paired with a separate `source_map_hash: Option<String>` at line 143 —
a hash-of-the-map field, not map data itself, listed for completeness not as a fifth map field),
`FfiVirtualFileResponse.source_map` (`:341`), `FfiIdeResponse.source_map` (`:387`), and
`FfiTscResponse.source_map` (`:397`) — four genuine map-data fields. (`types.rs:99` is a DIFFERENT,
unrelated field — `FfiCompileOptions.source_map: Option<bool>`, a request-side "should a map be
produced at all" flag, not map data, and is not part of this count.) The same string convention crosses
the FFI boundary too, so the "full extent" is wider than the compiler/LSP boundary alone.

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
