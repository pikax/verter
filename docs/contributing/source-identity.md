# Source and Map Identity

How a file, a position inside it, and a generated artifact keep one stable
identity across the parser, caches, scheduler, LSP and bundler outputs.
Every rule below is enforced by types or guards in the named files; this
page is the map, the code is the authority.

## Canonical file identity

Files enter the system through canonical paths: `VerterHost::upsert`
addresses a file by its canonical identity, the scheduler keys per-file
state (`FileNode`) by canonical file, and cache admission compares the
`(canonical, content_hash, parse_env_hash, resolve_env_hash, lib_env_hash)`
quintuple — an upsert that leaves it unchanged is a cache-state no-op.
The five environment-hash dimensions stay split (parse/resolve/type/lib
plus project identity); they must never be bundled into one project hash.
The store behind canonical post-parse artifacts is
`crates/verter_session/src/file_artifact_store.rs`
(`FileArtifactStore`); there is no second artifact authority.

Import resolution is context-sensitive and has exactly one owner:
`crates/verter_workspace` — the `WorkspaceAccess` trait
(`crates/verter_workspace/src/traits.rs`) with `NativeFs` as that crate's
`std::fs` adapter. Contributor code that resolves files or overlays must
go through that trait; there is no heuristic fallback resolution:
exact resolutions injected by the bundler/LSP win, then the project
resolver (tsconfig paths, aliases, `node_modules`), then nothing.

`NativeFs` is not the repository-wide filesystem boundary. Other crates
keep their own disk I/O for non-workspace concerns, including
`crates/verter_lsp/src/vue_assets.rs` (managed-tsgo Vue JSX adapter
files), `crates/verter_validation_probe/src/disk.rs` (CI corpus bytes),
and `crates/verter_audit/src/memory.rs` (`current_process_rss` reading
`/proc/self/statm` on Linux). Those are not import-resolution owners
and are not a license for a lint rule or contributor feature to open
files itself.

## Typed spans

All Rust span types live in `crates/verter_span/src/lib.rs`:

- `Span` — SFC-absolute byte offsets `[start, end)`; the only span type
  that serializes (serde/LSP/MCP/FFI boundaries). Analysis snapshots,
  diagnostics and CSS analysis all carry it.
- `RelativeSpan` — byte offsets relative to a base stored in context
  (CSS scanner internals, OXC binding extraction); same 8 bytes as `Span`,
  intra-crate only. `to_absolute(base)` rehydrates a `Span`.
- `PartialGeneratedSpan` → `GeneratedSpan` — unresolved vs resolved
  positions in generated TSX; `resolve(origin_span)` attaches the SFC
  origin after the provider answers.

There are deliberately no `From` conversions between these families, nor
between the source-side and generated-side coordinate wrappers
(`SourceByteRange` vs `GeneratedByteRange`, `LspPosition` vs `TsPosition`):
a TSX position can never be passed where a Vue-source position is
expected. Breaking this by adding a conversion is an API redesign, not a
convenience.

## Position encodings by layer

| Layer                     | Offset format         | Line/col base                    |
| ------------------------- | ---------------------- | -------------------------------- |
| OXC parser + `Span`       | UTF-8 byte offsets     | none (offsets only)              |
| FFI (`crates/verter_ffi/src/convert/offset.rs`) | UTF-16 code units | none (offsets only) |
| LSP protocol              | negotiated encoding   | 0-based line / 0-based character |
| VS Code API               | UTF-16 code units      | 0-based                          |
| Source maps (VLQ)         | —                      | 0-based line / 0-based column    |

The classic off-by-one: `PositionResolver` returns 1-based line/column;
subtract one before touching source maps or LSP positions. The LSP
negotiates its position encoding at `initialize()` (prefer UTF-8, then
UTF-32, then UTF-16) and everything that emits an LSP position — including
`SyncCoordinator` diagnostics — must use the negotiated one.

Generated↔source mapping is strict, not fuzzy:
`crates/verter_lsp/src/documents/position_map.rs` (`PositionMapper`)
answers `tsx_to_carrier`/`carrier_to_tsx` only when the query lies strictly
inside one mapped token run; no cross-token extrapolation, no snap-to-nearest.
Unmapped synthetic content (`_ctx.` prefixes and friends) returns `None`,
and callers must treat that as "no position", never guess.

## Stable hashes and cache identity

Content identity is layered so that unrelated edits cannot alias:

- Declaration-skeleton stability: each augmenter folds a
  `parse_stable_hash` over the decl skeleton (see
  `crates/verter_session/src/compile_fact_emission.rs`), so two parses
  with the same declaration inventory share facts even when whitespace
  differs.
- Query-identity keys are content-free: a semantic query key never embeds
  content hashes, version hashes or `fact_dep_signature`; version
  identity lives inside the cached value (rule R3 of the cache
  architecture, `CLAUDE.md` → Cache Architecture).
- Cache entries are validated on read against the caller's current
  `StoreView` — reuse is the default, recomputation is the exception, and
  a warm hit is only correct after that check.

If you are tempted to add a key "just to be safe", you are moving content
into identity; that is the exact anti-pattern the split exists to prevent.

## Source maps

Runtime codegen produces VLQ source maps (0-based lines/columns) via the
`CodeTransform` chunk engine — `crates/verter_compiler/src/code_transform/code_transform.rs`
— which tracks original positions through deferred edits. Chained and
composed maps (bundlers, downstream transforms) go through the assembly
stage: `crates/verter_compiler/src/assembly/map_compose.rs` and
`crates/verter_compiler/src/assembly/map_json.rs`. The IDE path is
different in kind: it does not ship a map file, it ships generated TSX
plus the strict `PositionMapper` contract above. Do not "recover" a
position by regex-scanning generated output; the mapper returning `None`
is a truthful answer.

## Related pages

- [Architecture contracts](./architecture-contracts.md) — owner boundaries
  and retained surfaces.
- [Query lifetimes and determinism](./query-lifetimes.md) — what lives for
  a session, a content version, or a request.
