---
name: position-encoding
description: "Position encoding, span types, coordinate systems, path normalization tables for Verter's multi-layer architecture (OXC, Rust, LSP, FFI, VS Code)"
---

# Position Encoding & Path Normalization Reference

## Typed Span Types (`verter_span` crate)

All Rust span types are defined in `crates/verter_span/src/lib.rs`. Each type enforces a specific coordinate system at compile time:

| Type | Meaning | Serde? | Used Where |
| ---- | ------- | ------ | ---------- |
| `Span` | SFC-absolute byte offsets `[start, end)` | **Yes** (`spanStart`/`spanEnd`) | Analysis types, diagnostics, CSS analysis, CodeTransform, Raw\* template data, CSS variable spans |
| `RelativeSpan` | Byte offsets relative to a base stored elsewhere | **No** | CSS scanner internals, OXC binding extraction (`Binding.span`) |
| `PartialGeneratedSpan` | Unresolved position in generated output (TSX) | **No** | TSGO response parsing before PositionMapper resolution |
| `GeneratedSpan` | Resolved mapping: generated position + SFC origin | **No** | TSGO diagnostics after resolution, codegen error mapping |

### Typed LSP / generated-TSX coordinate wrappers

Intra-process boundary newtypes (also in `verter_span`, **no serde**) used by the LSP `PositionMapper` and cross-file navigation stack. No `From` between source-side and generated-side types (no `From<GeneratedByteRange> for SourceByteRange`); `LspPosition`/`TsPosition` are distinct so a TSX position can never be passed where a Vue LSP position is expected.

| Type | Meaning |
| ---- | ------- |
| `SourceByteOffset` / `SourceByteRange` | byte offset / `[start,end)` range into the original `.vue` source |
| `GeneratedByteOffset` / `GeneratedByteRange` | byte offset / `[start,end)` range into the generated TSX |
| `GeneratedByteLen` | length of a generated-TSX content region (`content_offset` domain) |
| `SourceUtf16Offset` / `GeneratedUtf16Offset` | UTF-16 code-unit offsets (source / generated) |
| `LspPosition { line, character }` | 0-based Vue source position, LSP-negotiated encoding (Vue side) |
| `TsPosition { line, character }` | 0-based generated-TSX position (TSX side) |

## Typed Span Rules

1. **All data crossing a serialization boundary (serde, MCP, LSP custom protocol, FFI) MUST use `Span` (SFC-absolute).** `RelativeSpan`, `PartialGeneratedSpan`, `GeneratedSpan` don't implement `Serialize`/`Deserialize` — putting them in a serializable struct is a compile error. Convert with `to_absolute(base)` before serialization.
2. **Inter-crate stored types prefer `Span`.** Analysis snapshots, host results, diagnostic structs crossing crate boundaries use `Span`. `RelativeSpan` is intra-crate only (CSS scanner, OXC binding extraction).
3. **`RelativeSpan` is 8 bytes, same as `Span`.** Base offset lives in context (field on parent struct, function parameter). Value is compile-time type safety, not runtime data.
4. **`PartialGeneratedSpan` → `GeneratedSpan` via resolution.** Use `PartialGeneratedSpan` for raw TSGO byte offsets. After PositionMapper resolves SFC origin, use `partial.resolve(origin_span)` to get `GeneratedSpan`. For display use `generated_span.origin`.
5. **`PositionMapper` is a STRICT in-run mapper** (`crates/verter_lsp/src/documents/position_map.rs`). `tsx_to_vue(TsPosition) -> Option<SourceMapped>` and `vue_to_tsx(LspPosition) -> Option<GeneratedMapped>` return `Some` **only** when the query lies strictly inside ONE mapped token's run (the next token on the line starts strictly after the query). No cross-token extrapolation, no snap-to-closest-preceding fallback — unmapped/synthetic content (`_ctx.`/`$setup.` prefixes), gaps, or bridging into the next token return `None`. Within-run character precision IS preserved (in-run offset added to the run's mapped start), but only inside a single mapped run. A range maps only when BOTH endpoints resolve inside the SAME compatibility component — runs are pre-labelled at construction with a `component_id`; two runs share an id only when linked by an unbroken chain of runs contiguous in BOTH generated AND source space (same-line adjacency, or the multiline line-wrap equivalent). Generated-side adjacency alone is NOT sufficient: generated output relocates/repeats source (`MoveOriginal`, repeated v-model emission), so two generated-adjacent but source-discontiguous runs are DIFFERENT components and do not compose — `runs_compatible` is then an O(1) `component_id` comparison. Endpoints are therefore coupled (same component), never mapped independently. Guard: `crates/verter_lsp/tests/cases/position_mapper_strict.rs` (behavioural — including the generated-adjacent-but-source-discontiguous discriminator — + static `ban_cross_token_extrapolation`).

## Key APIs

- `Span::new(start, end)` / `RelativeSpan::new(start, end)` / `PartialGeneratedSpan::new(start, end)`
- `RelativeSpan::to_absolute(base: u32) -> Span` — add base offset
- `Span::to_relative(base: u32) -> RelativeSpan` — subtract base offset
- `PartialGeneratedSpan::resolve(origin: Span) -> GeneratedSpan` — resolve with SFC origin
- `GeneratedSpan::new(generated: Span, origin: Span)` — create resolved mapping directly
- `slice(&self, source: &str) -> &str` — on `Span`, `RelativeSpan`, `PartialGeneratedSpan`
- `From<oxc_span::Span>` for both `Span` and `RelativeSpan`
- `LspPosition::new(line, character)` / `TsPosition::new(line, character)`; `SourceByteRange::new(..)` / `GeneratedByteRange::new(..)` (typed LSP/TSX coordinate wrappers)
- **No `From` conversions between span types, nor between source-side and generated-side coordinate wrappers** — type safety enforced at compile time

## CSS Variable Analysis Spans

All CSS variable span fields use SFC-absolute `Span`:

| Type | Field | Meaning |
| ---- | ----- | ------- |
| `AnalyzedCustomProperty` | `name_span` | Span of `--name` in declaration |
| `AnalyzedCustomProperty` | `value_span` | Span of the value text after `:` |
| `CssVarReference` | `span` | Span of entire `var(...)` expression |
| `CssVarReference` | `name_span` | Span of variable name within `var()` |
| `CssVarFallback` | `span` | Span of fallback text within `var()` |
| `CssVarManipulation` | `span` | Span of DOM API call expression (e.g., `setProperty(...)`) |

Spans computed as `content_offset + local_offset` during CSS scanning, where `content_offset` is SFC-absolute byte offset of `<style>` block content start. For script-side `CssVarManipulation`, spans come from OXC adjusted by script block's SFC offset.

## Position Encoding Layers

| Layer | Offset Format | Line/Col Base | Description |
| ----- | ------------- | ------------- | ----------- |
| **oxc_span** | UTF-8 byte offset, relative to parse start | N/A (byte offsets only) | OXC parser spans are byte offsets from start of parsed source text |
| **verter `Span`** | UTF-8 byte offset, absolute for the document | N/A (byte offsets only) | All stored Rust spans (`span` fields in analysis types, `CodeTransform` positions) are byte offsets from start of SFC source |
| **verter `RelativeSpan`** | UTF-8 byte offset, relative to a base | N/A (byte offsets only) | CSS scanner internals (relative to style content start), OXC bindings (relative to expression start) |
| **PositionResolver** | N/A | **1-based** line, **1-based** UTF-16 column | `cursor/position.rs` — returns 1-based. Subtract 1 before passing to source maps or LSP |
| **Source maps** | N/A | **0-based** line, **0-based** column | VLQ-encoded. `source_map.rs` converts from PositionResolver via `(line - 1, col - 1)` |
| **LSP Protocol** | Negotiated (UTF-8/UTF-16/UTF-32) | **0-based** line, **0-based** character | `Position { line: 0, character: 0 }` = first char. `LineIndex` handles conversion |
| **VS Code API** | UTF-16 code units | **0-based** line, **0-based** character | `new Position(0, 0)` = first char. Matches LSP UTF-16 |
| **verter_ffi** | UTF-16 code units | N/A (byte offsets only) | NAPI/WASM boundary always communicates in UTF-16 offsets. Reference: `crates/verter_ffi/src/convert.rs:byte_offset_to_utf16()` |
| **verter_lsp** | Negotiated encoding (UTF-8, UTF-16, or UTF-32) | **0-based** | LSP negotiates encoding with client during `initialize()`. All positions use negotiated encoding |

## Line/Column Base Rules (CRITICAL — off-by-one bugs)

- **PositionResolver is 1-based** (`cursor/position.rs`): `offset_to_line_and_col()` and `offset_to_line_col()` return (1-based line, 1-based column). Always subtract 1 before passing to source maps or LSP.
- **Source maps are 0-based**: VLQ segments use 0-indexed lines and columns.
- **LSP is 0-based**: `Position { line: 0, character: 0 }` is the first character.
- **VS Code is 0-based**: `new Position(0, 0)` is the first character.
- **OXC/verter spans are byte offsets** — no line/column, no base conversion needed.

## LSP Position Encoding Negotiation

1. Server reads `capabilities.general.positionEncodings` from client during `initialize()`
2. Server picks best encoding: prefer UTF-8 (no conversion needed) > UTF-32 > UTF-16 > default UTF-16
3. Server announces selected encoding in `ServerCapabilities.position_encoding`
4. All LSP positions (standard and custom protocol) use the negotiated encoding

**CRITICAL**: The negotiated encoding MUST be used everywhere that produces LSP positions (diagnostics, hover ranges, completion positions, etc.). This includes `SyncCoordinator` which publishes diagnostics after typing stops — it shares the encoding via `Arc<RwLock<PositionEncodingKind>>` with the server. Default is UTF-16 (per LSP spec) until `initialize()` negotiates.

**Rust-internal code** should prefer UTF-8 byte offsets. **LSP boundary code** must convert to negotiated encoding. **JS/VS Code** always uses UTF-16.

**Standard LSP positions** (`line:character`): handled by `LineIndex` in `documents/line_index.rs`.
**Custom protocol data** (analysis spans): converted at LSP boundary before serialization.

## Encoding Conversion Reference

| Boundary | Pattern | Reference Implementation |
| -------- | ------- | ------------------------ |
| Rust internal → NAPI/WASM | `byte_offset_to_utf16()` | `crates/verter_ffi/src/convert.rs:281` |
| Rust internal → LSP client | Negotiated encoding conversion | `crates/verter_lsp/src/documents/mod.rs` |
| LSP Position → byte offset | `LineIndex::position_to_offset()` | `crates/verter_lsp/src/documents/line_index.rs` |
| Byte offset → LSP Position | `LineIndex::offset_to_position()` | `crates/verter_lsp/src/documents/line_index.rs` |
| TSGO response → byte offset | `position_to_offset()` | `crates/verter_lsp/src/tsgo/ipc.rs` (ASCII TSX only) |

## VS Code Extension

VS Code negotiates UTF-16. Analysis offsets arrive as UTF-16 code units from file start. JS string indexing is UTF-16 native, so `source.charCodeAt(offset)` and `source.length` work directly. Use shared `utf16OffsetToPosition()` from `packages/vue-vscode/src/utils.ts`.

## TSGO Integration

TSGO processes generated TSX which is always ASCII. For ASCII: byte offset == UTF-16 offset == UTF-32 offset. `position_to_offset()`/`offset_to_position()` in `ipc.rs` treat `character` as byte offset within line — correct for ASCII. Diagnostics from `publishDiagnostics` must resolve LSP positions to byte offsets using TSX content cache.

---

# Path Normalization

All file paths stored internally in **canonical ID** format. Normalization at entry boundaries (receiving paths); denormalization at exit boundaries (sending paths to external systems).

## Canonical ID Format

| Rule | Example |
| ---- | ------- |
| Forward slashes only | `c:/Users/dev/App.vue` (never `c:\Users\dev\App.vue`) |
| Lowercase Windows drive | `c:/Users/...` (never `C:/Users/...`) |
| No query strings | `App.vue` (not `App.vue?vue&type=script`) |
| No virtual suffixes | `App.vue` (not `App.vue._VERTER_.bundle.ts`) |
| UTF-8, no percent-encoding | `/home/user/my project/App.vue` (not `my%20project`) |

## Entry Boundaries (External → Canonical)

| Source | Function | Location |
| ------ | -------- | -------- |
| LSP client URI | `uri_to_canonical_id_from_str()` | `verter_lsp/src/documents/mod.rs` |
| File system / bundler path | `canonicalize_id()` | `verter_session/src/id.rs` |
| Bundler plugin | `generateComponentId()` | `packages/unplugin/src/core/compiler.ts` |
| CLI args | `path_to_file_uri()` | `verter_lsp/src/main.rs` |

## Exit Boundaries (Canonical → External)

| Target | Pattern | Location |
| ------ | ------- | -------- |
| LSP client (file URI) | `file:///` + canonical ID | `verter_lsp/src/features/definition.rs` |
| TSGO type provider | `path_to_file_uri()` | `verter_lsp/src/main.rs` |
| File I/O | `std::path::Path::new(canonical_id)` | OS handles both `/` and `\` on Windows |

## Implementation Rules

1. **Receive → normalize immediately**: Every path entering passes through `canonicalize_id()` or `uri_to_canonical_id_from_str()` before storage or comparison
2. **Store only canonical**: All maps, caches, analysis types use canonical IDs as keys
3. **Send → denormalize at the boundary**: Convert back to `file://` URIs or OS paths only when sending to external systems
4. **Never compare raw paths**: Always compare canonical IDs, never raw OS paths or URIs
