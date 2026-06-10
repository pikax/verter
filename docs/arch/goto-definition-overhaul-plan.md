# Verter go-to-definition — PHASED IMPLEMENTATION PLAN (rev 2)

> **Sequencing authority: `docs/arch/semantic-db-overhaul-unified-remaining-plan.md`** — this file is detail/reference only; the `D:/` paths and old SHAs/branches in it are HISTORICAL. This plan **OWNS its block range `G.P1`–`G.P6/7`** under the unified cross-plan sequencing (§3.1 there): `G.P1`→`G.P2` re-land early, `G.P3` after U2, `G.P4` after U3 (with the reconciled `CompileSnapshotId`), `G.P5`→`G.P6/7` after `G.P4`.

Implements the binding architecture in `D:/tmp/goto-def-architecture-decision.md` and adopts **every** binding OQ resolution and reviewer finding in `D:/tmp/consolidated-findings.md`. Breaking changes allowed. Four NEVERS bind every phase: **no shims, no legacy/dual paths, no stubs, no shortcuts.**

This revision (rev 2) resolves all 3 P0, all 10 P1, and the 5 P2 + NITs from round-1 review, and adopts the BINDING OQ-1..OQ-7 resolutions verbatim. A findings→resolution map is at the end (`§ FINDINGS RESOLUTION MAP`).

All file:line references were verified against the live tree on `refactor/semantic-db-overhaul`. Where a type/function does not yet exist it is flagged **(new)**; where the architecture doc referenced a type by an approximate name, the **real** repo name is used (e.g. canonical id is `CanonicalId = Arc<str>`, not `CanonicalFileId`; the combined-TSX cache struct is `CachedTsx`).

> **Grounded against HEAD 0b3a63894 (post-rebase, refactor/semantic-db-overhaul). Core nav/codegen files byte-identical to grounding; Phase-4 `virtual_file_pipeline.rs` / `cache_runtime` anchors refreshed.**

---

## CONTEXT (overall)

CTRL+CLICK go-to-definition lands on the wrong file/line. Root cause (4 subagents + 2 architecture consults + final codex decision): **two independent fault families**, not a source-map-model incapability.

1. **Mapping-substrate desync (compiler / IDE codegen).** Four IDE-only template emit sites collapse `prefix + user_identifier + suffix` into a single `out.overwrite(start, end, &format!(...))`, producing one `Chunk::Overwritten` whose source token sits at the prop start. The identifier interior is unreachable by strict lookup and the `PositionMapper` falls back to column-delta extrapolation through synthetic text. The correct chunk primitives (`InsertedMapped` + split overwrites) already exist and are used on the common paths (static-key `:prop`, v-for).

2. **Cross-file definition arbitration (LSP).** A three-path design (verter `definition_at_position`, TSGO merge, virtual-file branch) arbitrates by **file-suffix preference** and litters `Range::default()` (= line 0, col 0) for every target it cannot map. Target position mapping reads the **open-document registry only**, so a closed target `.vue` falls to `(0,0)`. `.vue` default imports route through a `find_export_span` heuristic that returns the **first script binding** (or `(0,0)`) instead of the component anchor.

End state (architecture): **one chunk graph, one V3 map, strict Option-returning lookup, typed coordinates, one navigation engine, host-sourced snapshot-validated target mappers, anchor-based `.vue` targets, identity-based dedup.** No second mapping table; `CodeTransform`/V3 stays authoritative.

### The single-engine boundary (OQ-1, BINDING)

`DefinitionEngine` is a **navigation orchestrator**, NOT a type resolver. It must obey the "exactly one type-resolution engine" rule by **never** issuing a typed-IR query for declaration-site lookup:

- **Declaration-site (location-only) lookup** goes through a new **`TsNavigationBackend`** trait whose **location-only methods** (definition + the sibling references/rename/code-action surfaces; see Phase 5/5A) return **opaque locations** (`TsDefinitionSpan`/`TsRenameEdit`/`TsCodeActionEdit`) — NO `TypeExpr`, `SemanticNodeId`, member sets, quickinfo, component-meta, projections, expansions, skeletons, or generic instantiations. Implemented by `verter_lsp::tsgo::TsgoNavigationBackend` over tsgo `getDefinitionAtPosition` (and the sibling `getReferences`/`getRenameLocations`/`getCodeActions`).
- **Type/member-valued answers** (only needed by **type-definition**, Phase 7) route through the shared `SemanticQueryKey → ProjectSemanticDispatch → SemanticGraphStore` dispatch — the same single engine every other consumer uses. **No new `SemanticQueryKey` declaration-site mode is introduced.**

This split is mechanically guarded (four guards, see Phase 5 / Phase 7).

### Verified substrate inventory (what exists vs. what is new)

**Exists (reuse, do not reinvent):**
- `crates/verter_span/src/lib.rs` — `Span` (SFC-absolute, serde), `RelativeSpan`, `PartialGeneratedSpan`, `GeneratedSpan`. `.vue` eval sources are **position-preserving** (`IndexedReady.eval_source` doc, `project_type_store.rs:136-142`), so SFC-absolute `Span` is the correct carrier for anchors and same-file targets.
- `Chunk::InsertedMapped { content, source_start, content_offset }` — `crates/verter_compiler/src/code_transform/chunk.rs:35-39`; emission at `crates/verter_compiler/src/code_transform/source_map.rs:201-244`.
- `CodeGenOutput::{overwrite, prepend_static, prepend_alloc, prepend_alloc_mapped, prepend_alloc_mapped_with_offset, move_wrapped}` — all in `crates/verter_compiler/src/template/code_gen/types.rs` (84/98/104/113/123/148).
- Canonical correct IDE emitters to **mirror**: static-key `:prop` split — `crates/verter_compiler/src/ide/template/props.rs:504-564`; v-for iterable/params — `crates/verter_compiler/src/ide/template/directives.rs:150-180`; mapped emits in `crates/verter_compiler/src/ide/template/mod.rs:2225-2283`.
- IDE prop emit `fn` anchors (`crates/verter_compiler/src/ide/template/props.rs`, verified): `process_element_props` (`:63`), `process_v_bind` (`:401`), `process_v_on` (`:579`), `process_v_model` (`:826`), `process_v_html` (`:1024`), `process_v_text` (`:1040`), `resolve_prefixed_expr` (`:1279`), `resolve_prefixed_dynamic_arg` (`:1293`). (The per-construct call-site lines in the round-1 caller enumeration — e.g. `process_v_bind:422/436/465/505` — are the reviewer-verified caller offsets; re-confirm at impl time as edits shift them.)
- `collect_binding_patches` — `crates/verter_compiler/src/template/code_gen/binding.rs:297-342` (**SHARED** with VDOM/Vapor runtime — flat-string contract MUST NOT change).
- `build_prefixed_expr` — `crates/verter_compiler/src/template/code_gen/vapor/interpolation.rs:61-172` (**SHARED** — flat-string contract MUST NOT change).
- `VerterHost::ensure_compiled(canonical_id, &CompileProfile) -> Result<(), HostError>` — `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs:461`. Warm-hit identity is `semantic_hash` + style/content override hashes + profile, AND it validates cross-file facts via `compile_slot_facts_validate` (`:560`).
- `VerterHost::get_ide(canonical_id, &CompileProfile) -> Option<IdeResponse>` — `virtual_file_pipeline.rs:1355`; reads `CachedTsx` via `CompileOutputNodeFactValidatedSession::peek_tsx` (`crates/verter_session/src/cache_runtime/compile_output_node.rs:576`, returns `Option<CachedTsx>`). Direct `compile_slots` access is now forbidden — all reads route through `CompileOutputNodeFactValidatedSession`'s typed methods. (Design unchanged: `CachedTsx` remains the `CompileSnapshotId` publish struct and flows through `get_ide`'s `IdeResponse` automatically.)
- `IdeResponse { code, source_map, is_jsx, destructured_block }` — `crates/verter_session/src/types.rs:1732-1741` (no snapshot field yet).
- `CachedTsx { code, source_map, is_jsx, destructured_block }` — `crates/verter_session/src/types.rs:1723-1727` (the combined-TSX cache value; built at `virtual_file_pipeline.rs:1851-1860`). **This is the compile-cache publish point for `CompileSnapshotId` (OQ-4).** The `CachedTsx` flows through `CompileOutputValue.tsx: Option<CachedTsx>` (`cache_runtime/compile_output_node.rs:130`), so the added `CompileSnapshotId` propagates through the cache-runtime node for free.
- `FileAnalysisSnapshot` — `crates/verter_session/src/types.rs:1224-1279` (carries `template: Option<Arc<TemplateAnalysisSnapshot>>` at `:1244`, `bindings`, `macros`, `export_signatures`); `ScriptAnalysisSnapshot` — `crates/verter_semantic/src/analysis/types.rs:162-253`; `AnalyzedMacro` — `types.rs:1306-1365`.
- `IndexedReady` — `crates/verter_session/src/project_type_store.rs:95-164` (struct + doc; `whole_hash` is the first field at `:115`) — the canonical post-parse artifact; carries `snapshot: Arc<FileAnalysisSnapshot>`, `script_analysis`, `cached_parse: Option<Arc<ParsedSfc>>`, `eval_source`. Published via `FileArtifactStore::insert` (`file_artifact_store.rs:960`); built in `host_manage/prepared_decl.rs:1708` (`ensure_indexed_ready`, `:1372`) and `host_manage/overlay_materialize.rs:533`. **This is the anchor publication point for `SfcComponentAnchor` (OQ-3).**
- `CanonicalId = Arc<str>` — `crates/verter_session/src/capture_token.rs:64` (the real canonical-id type used across the host).
- `TypeProvider::get_definition(path, offset) -> Vec<TypeLocation>` — `crates/verter_lsp/src/extension_provider.rs:352`; `TypeLocation { path: String, start: u32, end: u32 }` — `crates/verter_type_runtime/src/protocol.rs:130-135` (byte offsets, no snapshot).
- tsgo project/file sync registry — `crates/verter_lsp/src/tsgo/project_sync.rs` (the synced-file registry OQ-4 stores `CompileSnapshotId` on); `crates/verter_lsp/src/sync_coordinator.rs`.
- Nav handlers — `crates/verter_lsp/src/server/nav_features.rs`: `handle_goto_definition` (`:779-994`), `handle_goto_type_definition` (`:997-1109`), `handle_references` (`:1112`), `handle_rename` (`:1277`); audit wrappers in `nav_features_audit.rs` (`handle_goto_definition_with_audit:89`, `handle_references_with_audit:125`, `handle_rename_with_audit:153`).
- tsgo merge arbitration — `crates/verter_lsp/src/tsgo/merge.rs`: `resolve_vue_tsx_range` (`fn` at `:647`), `merge_definitions_with_barrel_resolver` (`:725`), `merge_references` (`:874`), `merge_rename_locations` (`:943`), `merge_code_actions` (`:1102`), `merge_semantic_tokens` (`:1171`, with mapper call-sites `:1183`/`:1198`), `merge_inlay_hints` (`:1269`, mapper call-site `:1283`). (These `fn`-definition lines are stable anchors; the per-`Range::default()` call-site lines inside them are volatile and re-verified at impl time.)
- Semantic dispatch single entry — `SemanticGraphStore`/`ProjectSemanticDispatch::execute` via `execute_cooperative` (`crates/verter_session/src/semantic_query_memo/mod.rs:2197`). **OQ-1 guard-4 runtime hook (NIT-2):** the per-request dispatch counter — `with_active_capture(|t| t.record_dispatch(key, hit))` at `semantic_query_memo/mod.rs:2359` (warm) and the cold-path `t.record_dispatch(&key, false)` at `:2416`, plus `record_dispatch_warm(key)` at `:2353` — is the concrete count-of-dispatches surface a go-to-def test asserts is ZERO (alternatively the `AuditObserver` counter). (`verter_audit::current_observer()` at `:3818` is a doc-comment/prose mention, NOT the counter — kept only as an observability-doc reference.)
- Audit substrate — `verter_audit` `RequestKind` (`Lsp`, `ComponentMeta`, `TypeResolution`, …), `AuditObserver`, `StructuredAuditEvent` (`crates/verter_audit/src/{lib.rs,batch.rs,structured_event.rs}`).
- Guard registries: `crates/verter_session/tests/architecture_guards.rs`; the R6 meta-guard `every_critical_rule_in_docs_has_registered_guard` lives at **`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`** (corrected path; the round-1 plan and a stale `CLAUDE.md` pointer both drifted — see NIT fixes).

**Does NOT exist (create; scoped per-phase):**
- Typed coordinate wrappers `SourceByteOffset`, `GeneratedByteOffset`, `SourceUtf16Offset`, `GeneratedUtf16Offset`, `SourceByteRange`, `GeneratedByteRange`, `GeneratedByteLen`, `LspPosition`, `TsPosition` — **new** (Phase 1; `verter_span`). (`Hash16 = [u8;16]` already exists at `crates/verter_session/src/types.rs:12` — reused, not created.)
- `EmitOp` typed IDE-emit enum + `emit.rs` + `JsxBindingValue` + `EmitText` — **new** (Phase 2; `verter_compiler::ide::template::emit`).
- A producer-side **`defineOptions({ name })` name-span field** on `AnalyzedMacro` (or `AnalyzedOptionsApi`) — **new** (Phase 3; `verter_semantic`).
- `SfcComponentAnchor` / `SfcAnchorKind` / `TemplateAnalysisState` — **new** (Phase 3).
- `CompileSnapshotId(u128)` — **new** (Phase 4; `verter_session`). The nav-validity compile warm-hit identity (see §3.1.2). Named `CompileSnapshotId` to avoid colliding with U5's net-new cache-entry-pin `SnapshotId(u64)` (both land in `verter_session`).
- `TargetIdeContext`, `TargetSourceContext` — **new** (Phase 4; `verter_session`).
- `TsNavigationBackend` trait + `TsDefinitionSpan` — **new** (Phase 5; trait in `verter_session::navigation`, impl `TsgoNavigationBackend` in `verter_lsp::tsgo`).
- `verter_session::navigation::definition` core: `DefinitionRequest`, `DefinitionTarget`, `DefinitionQuery`, `DefinitionEngine`, `DefinitionSymbolKind`, dedup identity, target normalization, barrel terminalization — **new** (Phase 5; LSP-FREE per OQ-5).
- `CompilerLocation` (a coordinate-space + provenance enum: `Generated { span: GeneratedByteRange, snapshot_id }` | `RealSource { span: SourceByteRange, source_hash: Hash16 }`) and `TargetProvenance` (`GeneratedMapping(CompileSnapshotId)` | `HostSource(Hash16)` | `LiveSameFile`) — **new** (Phase 5/6-7; P1-1 — every compiler-derived location carries intrinsic provenance for its coordinate space).

### Phase sequencing rationale (P0-3 resolved)

Codex's 7-step shape is the backbone, with the **P0-3** correction: **no phase deletes a symbol whose callers still exist.** Concretely:

- Phase 1 (mapper) lands first: it changes the contract every cross-file consumer relies on; it fixes **all** consumers in the same phase so the tree always compiles.
- Phase 2 (compiler emit) is independent of Phase 1.
- Phases 3-4 build the cross-file substrate (anchors + host-sourced snapshot-validated mappers) consumed by Phase 5.
- **Phases 6 and 7 are MERGED** into a single landable change (**Phase 6/7**) that routes *every* nav surface (definition, type-definition, references, rename, code-actions) through the engine **and** deletes the legacy arbitration (`merge_definitions_with_barrel_resolver`, `resolve_vue_tsx_range`, etc.) in the **same** change. This is mandatory because those functions' last callers are spread across definition (Phase 5/6) and type-def/refs/rename (Phase 7); deleting them before the type-def/refs/rename handlers are rewired would leave a caller (a retired-symbol guard could not pass mid-sequence). The merged Phase 6/7 deletes each symbol only after its last caller is gone, in one commit.

Each phase is **independently landable** (compiles, full suite green) and **independently verifiable** (named discriminating tests + guards). Where a phase replaces a path, it deletes the old one in the same phase.

### Blast radius (crates touched)

- `verter_span` — Phase 1 (coordinate wrappers).
- `verter_compiler` — Phase 2 (IDE emit sites + `EmitOp`/`emit.rs`).
- `verter_semantic` — Phase 3 (defineOptions name-span field; `SfcComponentAnchor`/`SfcAnchorKind` types live here or in `verter_session` per OQ-5 — see Phase 3).
- `verter_session` — Phase 3 (anchor production at `IndexedReady` + `TemplateAnalysisState`), Phase 4 (`CompileSnapshotId` on `CachedTsx`/`IdeResponse`, `target_ide_context`/`TargetSourceContext`), Phase 5 (`navigation::definition` core + `TsNavigationBackend` trait), Phase 6/7 (engine generalization).
- `verter_lsp` — Phase 1 (mapper consumers), Phase 5 (`TsgoNavigationBackend` + render-only wiring), Phase 6/7 (deletions + all nav surfaces).
- TS/e2e — committed fixture workspace `packages/vue-vscode/e2e/fixtures/goto-definition/` + `definition.test.ts`/`references.test.ts`/`rename.test.ts`/type-definition assertions — Phases 2/5/6-7.
- Docs/skills — `position-encoding`, `compiler-codegen`, `host-session`, `component-meta`, `type-resolution`; `CLAUDE.md` CRITICAL section + the guard registry at `tests/g_misc0/critical_rules_have_guards.rs`.

---

## PHASE 1 — Harden `PositionMapper`: typed coordinates + strict cross-token lookup (keep within-run precision)

### Context
The mapper is the substrate every cross-file consumer depends on. **(P1-E correction)** The two public methods are **already `Option`-returning**; the real defect is (a) they take/return raw `(line, column)` and (b) they apply *cross-token* extrapolation: `tsx_to_vue` adds a column delta when the query is past a token (`crates/verter_lsp/src/documents/position_map.rs:50-53`), runs a nearest-previous backward scan (`:56-109`), and `vue_to_tsx` snaps to the closest preceding token then adds a delta (`:154-158`). This phase makes inputs/outputs **typed** and deletes **cross-token** extrapolation while **preserving within-run character precision** (P1-D).

It must land first because it changes the typed signature every caller relies on; every caller is updated in the same phase to keep the tree compiling. It intentionally does **not** yet fix the four desync emit sites (Phase 2): after Phase 1 those four constructs return `None` (feature drop) instead of a wrong position — a strictly-better failure mode and the precondition that makes Phase 2's regression tests discriminate.

### Changes

**1A. New typed coordinate wrappers — `crates/verter_span/src/lib.rs` (append).**
Add newtypes (each `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`, no serde — intra-process boundary types):
```rust
pub struct SourceByteOffset(pub u32);       // byte offset into .vue source
pub struct GeneratedByteOffset(pub u32);    // byte offset into generated TSX
pub struct GeneratedByteLen(pub u32);       // length of generated content (content_offset domain)
pub struct SourceUtf16Offset(pub u32);
pub struct GeneratedUtf16Offset(pub u32);
#[derive(...)] pub struct SourceByteRange    { pub start: SourceByteOffset,    pub end: SourceByteOffset }
#[derive(...)] pub struct GeneratedByteRange { pub start: GeneratedByteOffset, pub end: GeneratedByteOffset } // byte range in generated TSX
#[derive(...)] pub struct LspPosition { pub line: u32, pub character: u32 } // 0-based, negotiated encoding (Vue side)
#[derive(...)] pub struct TsPosition  { pub line: u32, pub character: u32 } // 0-based, TSX coordinates
```
Provide only total, lossless conversions (no hidden `as` truncation). NO `From` between source-side and generated-side types (mirrors the existing `verter_span` "no `From` between span types" rule) — in particular **no `From<GeneratedByteRange>` for `SourceByteRange`** so a generated span can never be silently stored as a source span. `LspPosition`/`TsPosition` are distinct so a TSX position can never be passed where a Vue LSP position is expected. `GeneratedByteRange` is the coordinate-space carrier for every generated-TSX nav span (P1-1): backend answers and `CompilerLocation`/`DefinitionTarget` use it (paired with `CompileSnapshotId`) for the generated space and `SourceByteRange` (paired with `Hash16`) for the real-source space — the two never mix.

**1B. Retype + tighten `PositionMapper` — `crates/verter_lsp/src/documents/position_map.rs`.**
- Retype the two public methods (already `Option`):
  - `pub fn tsx_to_vue(&self, pos: TsPosition) -> Option<SourceMapped>` where `SourceMapped` carries `LspPosition`.
  - `pub fn vue_to_tsx(&self, pos: LspPosition) -> Option<GeneratedMapped>` where `GeneratedMapped` carries `TsPosition`.
  - (Names illustrative; the binding requirement is typed-wrapper inputs/outputs and `Option` returns.)
- **DELETE only cross-token extrapolation:**
  - `:56-109` — the nearest-previous backward-scan fallback (`best`/`best_dst_col` loop and the "unmapped token between" guard). Cross-token; deleted.
  - The **unconditional** column-delta add path. (P1-D) Where today `:50-53` adds a delta whenever the query is past the token, REPLACE it with a **within-run guarded delta** (below) — do NOT delete within-run precision wholesale.
- **KEEP within-run precision (P1-D, REPLACE-not-delete):** for a query strictly inside a *single mapped run*, the source column = `token.src_col + (query.col - token.dst_col)` is applied **only when** (1) the enclosing token has a source id AND (2) the **next** token on the line starts strictly after `query.col` (the query is within this token's run, not bridging into the next/unmapped token). This preserves the architecture's round-trip invariant for exact mapped user text and keeps the retained interior tests green. Character-level precision is allowed **only within one mapped run**, never across an unmapped/synthetic token boundary.
- New `tsx_to_vue` logic: `lookup_token`; map when the token has a source id and the query falls inside that token's mapped run per the within-run rule above. A query that lands in unmapped / `Inserted` / `InsertedMapped`-prefix content, or in a gap between tokens, → `None`.
- New `vue_to_tsx` (symmetric, P1-D + **P2-3**): the source position must fall inside a mapped token's source run; within-run character delta applied under the same two-condition guard; otherwise `None`. **No snap-to-previous.** The snap-to-closest-preceding behaviour is NOT just the trailing delta — it is the **whole lookup body at `position_map.rs:124-150`** (the `for token` loop that selects the token with the **highest** `src position <= target`, i.e. the closest preceding token even when the target is in a different/unmapped run), plus the trailing column-delta at `:154-158`. **Rewrite the entire `:124-158` lookup to require IN-RUN CONTAINMENT** — return `Some` only when the target source position lies **inside one mapped token's own source run** (next token on the line starts strictly after the target), and apply the within-run delta only then; any target that would otherwise "snap" to a preceding token (because it falls in a gap or a later/unmapped run) returns `None`. Do NOT merely delete the `:154-158` delta and keep the `:124-150` snap loop — that would still snap to the previous token.
- Half-open boundary rule: a range maps only when **both** endpoints resolve inside compatible mapped spans (the consumer composes `tsx_range_to_vue_range` from two endpoint lookups — already the case in `merge.rs`, `fn` at `:138`).

**1C. Update every `PositionMapper` caller to the typed contract** (P1-E — COMPLETE verified set, no "etc."):
- `crates/verter_lsp/src/tsgo/merge.rs`:
  - `vue_position_to_tsx_offset`, `vue_position_to_tsx_offset_validated`, `find_exact_roundtrip_offset` — wrap inputs in `LspPosition`, consume typed `Option`.
  - `tsx_range_to_vue_range` (`fn` at `:138`) — adapt to typed endpoints (already `Option`).
  - `merge_definitions_with_barrel_resolver`, `merge_references`, `merge_rename_locations`, `merge_document_highlights` — propagate typed `Option`.
  - **`merge_semantic_tokens` (`merge.rs:1183`, `:1198`)** and **`merge_inlay_hints` (`merge.rs:1283`)** — direct mapper callers; retype (these were missing in round 1 and the tree would not compile without them).
  - `resolve_vue_tsx_range` (`fn` at `:647`; its `.unwrap_or_default()` at `:681`) — compiles against the new signatures in Phase 1 (its `.unwrap_or_default()` survives until it is deleted in Phase 6/7).
- `crates/verter_lsp/src/server/nav_features.rs`: `handle_goto_definition`, `handle_goto_type_definition`, `handle_references`, `handle_rename` — adapt `merge::vue_position_to_tsx_offset_validated(...)` calls and virtual-file `offset_to_position(...)` results to typed/`Option`.
- `crates/verter_lsp/src/server/provider_state.rs`: `type_provider_context`, `external_ide_context` — construct typed coordinates from `LineIndex`.
- `crates/verter_lsp/src/features/definition.rs`: `span_definition`, `definition_at_position` — consume typed `LineIndex` outputs where they feed the mapper.
- **`crates/verter_lsp/src/integration_tests.rs`** — the **17** call sites using the old `(line, column)` signature (verified count of `tsx_to_vue(`/`vue_to_tsx(` in that file; e.g. direct `tsx_to_vue(line, col)` / `vue_to_tsx(line, col)` calls). Retype each to construct `TsPosition`/`LspPosition`. (Enumerated by the implementer via a grep for `tsx_to_vue(`/`vue_to_tsx(` across `crates/verter_lsp/src/**`; ALL must compile — the typed signature is the forcing function.) **(NIT-3)** The mapper-method call sites in `verter_type_runtime/src/tsgo/ipc.rs` are out of scope: they live under `#[cfg(all(test, feature = "__lsp_tests"))]` (`ipc.rs:20`/`:220`/`:478`/`:844`/`:2967`) and `__lsp_tests` (defined empty at `verter_type_runtime/Cargo.toml:24`) is never enabled — so those copies do not compile in `cargo test --workspace --tests` and are NOT touched. The "all callers compile" forcing-function is therefore correctly scoped to `crates/verter_lsp/src/**` only.

### Legacy Deletions
- `position_map.rs:56-109` — `tsx_to_vue`'s nearest-previous backward-scan fallback + intervening-unmapped guard (the whole `best` block). **Cross-token; deleted.**
- The **unconditional/cross-token** column-delta add at `:50-53`. **Replaced** by the within-run guarded delta (NOT deleted wholesale — P1-D).
- **(P2-3)** `vue_to_tsx`'s snap-to-closest-preceding lookup — the **whole `:124-150` loop body** (which picks the highest `src <= target` token, snapping to the previous token across runs) **and** the trailing snap-delta at `:154-158`. **Rewritten** to require in-run containment (return `None` instead of snapping to a preceding token); the within-run delta applies only inside one mapped run. NOT a delete-the-delta-only change — the snap loop itself is the defect.
- Any caller assumption that the mapper returns a non-`Option` "best effort" position (enforced by the typed signature flip; the compiler is the guard).

### Tests (write FIRST; FAIL pre-change, PASS post-change)
Rust unit tests in `position_map.rs` `#[cfg(test)]` (the existing `build_source_map_with_unmapped` helper models mapped+unmapped tokens). Retype existing tests to the typed API and ADD:
- `test_tsx_to_vue_inside_prefix_returns_none` — query at the `_ctx.`/`$setup.` prefix columns → `None`. (Retarget existing `test_prepended_text_inside_prefix_returns_none`.)
- `test_tsx_to_vue_cross_token_no_extrapolation` — REPLACE `test_tsx_to_vue_between_tokens` (`:237`): a query 5 columns past a mapped token, in a **gap with no covering run** → `None` (today extrapolates). Discriminating for deleting `:56-109` + the cross-token delta.
- `test_tsx_to_vue_within_run_character_precision` — **(P1-D, discriminating the other way)** a query inside a single mapped multi-char identifier run (e.g. column 3 of a 6-char mapped token) → maps to the corresponding source column. FAILS if within-run precision was deleted; PASSES with the guarded delta. (Retain/retype `test_prepended_text_roundtrip_character_level`.)
- `test_tsx_to_vue_unmapped_synthetic_interior_returns_none` — overwritten-punctuation interior → `None`.
- `test_vue_to_tsx_unmapped_source_returns_none` — REPLACE `test_vue_to_tsx_offset_within_token_range` (`:276`): source position in a gap → `None`.
- `test_vue_to_tsx_within_run_character_precision` — symmetric within-run precision for `vue_to_tsx`.
- `test_roundtrip_exact_mapped_text_identity` — exactly-mapped identifier: `source→generated→source == original`.
- `test_half_open_one_past_end` — one-past-end of a mapped run resolves only when the end endpoint is inside a compatible span; else `None`.
- `test_crlf_mapping`, `test_tabs_mapping`, `test_multiline_mapped_expression` — **(P2-B)** CRLF line endings, tab-indented source, and a mapped expression spanning multiple TSX lines all round-trip exactly. Discriminating: a line-index/offset bug under CRLF/tabs yields a wrong column.
- UTF-16 / surrogate / astral / emoji / non-ASCII roundtrips — keep existing `test_utf16_*`, retype.

### Architecture guards
- `crates/verter_lsp/tests/position_mapper_strict.rs` (new): `mapper_methods_take_typed_coordinates_and_return_option` — a constructed mapper with a single unmapped token yields `None` for an interior query (discriminating: a re-introduced cross-token fallback makes this `Some`); a single mapped multi-char run yields `Some` with correct within-run column (discriminating: deleting within-run precision makes this `None`/wrong).
- `ban_cross_token_extrapolation` (static, in `architecture_guards.rs`): scan `crates/verter_lsp/src/documents/position_map.rs` source for the deleted patterns and assert absent — `tsx_to_vue`'s `best_dst_col`/nearest-previous loop markers, **`vue_to_tsx`'s snap-to-closest-preceding markers (`best_src_col`/`best_src_line` "highest src ≤ target" loop, P2-3)**, and any unconditional `+= column -`/`pos.column +=` delta outside the within-run-guarded branch. (Scoped NOT to ban the within-run guarded delta.) Discriminating: re-introducing the `vue_to_tsx` snap loop (a `best_src_col` that selects a preceding token) fails the scan.

### Verification
```
cargo nextest run --workspace 2>&1 | tee /tmp/p1.txt
cargo test -p verter_session --tests 2>&1 | tee -a /tmp/p1.txt
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
```
Expected: full suite green. The four desync constructs now return **no result** rather than a wrong one. Common-path navigation (static `:prop`, plain interpolation, script bindings) unchanged; within-run interior precision preserved (the retained interior tests pass).

---

## PHASE 2 — Replace IDE-only prefixed-expression emission with the typed `EmitOp` substrate

### Context
The four confirmed desync sites bake `prefix + identifier(+suffix)` into one `out.overwrite(prop.start, prop_end, &format!(...))`, producing one `Chunk::Overwritten` mapped at the prop start. With Phase 1's strict mapper these now drop to `None`. This phase introduces a **typed** emit substrate (OQ-2) so the identifier maps exactly and the bug is unrepresentable. The correct pattern already exists in the same module (`props.rs:504-564`, `directives.rs:150-180`): split synthetic boundaries (`overwrite`/`prepend_static`) and emit the user expression via `prepend_alloc_mapped_with_offset` / `InsertedMapped` so its source token survives.

The hardest case is **native v-model**, which emits the resolved expression **2-3 times** in one `format!`. Per the architecture, each occurrence becomes its **own** mapped emission pointing at the **same** source span; all assignment/call/punctuation text is synthetic/unmapped.

### Changes

**2A. New `crates/verter_compiler/src/ide/template/emit.rs` (OQ-2, P1-A typed).**
```rust
pub enum EmitText { Static(&'static str), Borrowed(&'a str), Owned(String) } // avoids needless allocs
pub enum EmitOp {
    InsertUnmapped { at: SourceByteOffset, text: EmitText },
    InsertMapped   { at: SourceByteOffset, text: EmitText, source_start: SourceByteOffset, content_offset: GeneratedByteLen },
    PreserveOriginal { source: SourceByteRange },
    OverwriteSyntheticBoundary { source: SourceByteRange, text: EmitText, anchor: SourceByteOffset },
    MoveOriginal { source: SourceByteRange, at: SourceByteOffset },
}
pub struct JsxBindingValue<'a> {
    pub source_expr: SourceByteRange,       // the user expression span (trimmed)
    pub prefix: Option<&'a str>,            // e.g. "_ctx." / "$setup." (synthetic)
    pub suffix: Option<&'a str>,            // synthetic trailing text, if any
    pub occurrences: u8,                    // 1 for most; >1 for v-model
    pub bindings: &'a [BindingPatch],       // sub-expression mapping (collect_binding_patches output)
}
pub fn emit_jsx_binding_value(out: &mut CodeGenOutput, at: SourceByteOffset, v: &JsxBindingValue);
```
**(P1-A) Coordinates are the typed wrappers from Phase 1 (`SourceByteOffset`/`GeneratedByteLen`/`SourceByteRange`), not raw `u32`.** `MoveOriginal` + the `anchor` field are present (verbatim architecture). The four desync sites need no `MoveOriginal`; it exists for parity with the architecture and for future movers — documented as such, not a silent deviation.

**(P1-B, P1-C) Exact per-op lowering to `CodeGenOutput` (no contradictions):**
- `InsertUnmapped { at, text }` → `prepend_static(at, text)` (or `prepend_alloc(at, text)` for `Owned`). Produces an **`Inserted`** (unmapped) chunk → maps to `None`. **Use this for a separate prefix; then the adjacent `InsertMapped` carries `content_offset = 0`** (the mapped text is the bare identifier).
- `InsertMapped { at, text, source_start, content_offset }` → `prepend_alloc_mapped_with_offset(at, source_start, text, content_offset)` → emits a `Chunk::InsertedMapped { content: text, source_start, content_offset }`. The mapped content begins at byte `content_offset` within `text`; bytes `[0, content_offset)` are unmapped. **Use `content_offset = prefix.len()` ONLY when `text` itself is `prefix+identifier`; in that case do NOT also emit a separate `InsertUnmapped(prefix)`.** (P1-B: the two forms are mutually exclusive — the plan picks **separate-prefix + content_offset=0** for v-html/v-text/`:[key]`/v-model so the prefix is an explicit `Inserted` chunk and the identifier maps from byte 0.)
- `PreserveOriginal { source }` → **(P2-1) a PURE NO-OP over `source`** — emit **nothing**: no `overwrite`, no insert, no delete. The original `source` bytes stay an `Original` (1:1 mapped) chunk verbatim. `PreserveOriginal` carries ONLY `source` and lowers using only `source` (no `prop.start`/`prop_end`/`suffix_text` — those are absent from the variant). **All visible synthetic prefix/suffix and every boundary delete are separate `OverwriteSyntheticBoundary` ops the caller emits around the preserved span** (e.g. leading `OverwriteSyntheticBoundary(prop.start..source.start, "innerHTML={")`, trailing `OverwriteSyntheticBoundary(source.end..prop_end, "}")`). `PreserveOriginal` itself never writes synthetic text — that is the invariant that keeps the preserved identifier 1:1 mapped and all synthetic boundaries unmapped.
- `OverwriteSyntheticBoundary { source, text, anchor }` → **(P1-C) lowered as an UNMAPPED replacement, NOT a mapped overwrite.** Lower as `overwrite(source.start, source.end, "")` (delete the original bytes) **+** `prepend_static(source.start, text)` (insert synthetic visible text as an **`Inserted`** chunk). The synthetic text (`innerHTML=`, `{`, `}`, `]: `, `=>`, `=`, `(` , `)`) therefore carries **no** source mapping → maps to `None`, satisfying the None-on-synthetic tests. The `anchor` field is recorded only for *named* semantic anchors that intentionally remain navigable (none of the four desync sites use it; it is the architecture's explicit "named anchor" affordance). A bare `out.overwrite(start, end, text)` (the `Chunk::Overwritten` whose source sits at `start`) is **never** used for synthetic boundaries because that would map the boundary start — which is exactly the bug.
- `MoveOriginal { source, at }` → `move_wrapped(...)` (the existing mover); preserves the original chunk's mapping at its new location.

**2B. Rewrite `process_v_html` — `crates/verter_compiler/src/ide/template/props.rs` (`fn` at `:1024`; the single-overwrite emission inside it).**
Replace the single `out.overwrite(prop.start, prop_end, &format!("innerHTML={{{}}}", resolved))` with `emit_jsx_binding_value`: `OverwriteSyntheticBoundary(prop.start..value_start, "innerHTML={")` + the expression (prefix as `InsertUnmapped`, identifier as `InsertMapped@source_start, content_offset=0`, sub-expressions via `bindings` from `collect_binding_patches`) + `OverwriteSyntheticBoundary(value_end..prop_end, "}")`. When the resolved expression is the unchanged original (no prefix rewrite needed), the middle op is `PreserveOriginal(value_span)` — a pure no-op (P2-1); the two `OverwriteSyntheticBoundary` ops still emit `innerHTML={` and `}` (the only visible synthetic text), so the identifier stays 1:1 mapped and both boundaries map to `None`.

**2C. Rewrite `process_v_text` — `props.rs` (`fn` at `:1040`).** Same as 2B with `textContent={` / `}`.

**2D. Rewrite dynamic-key bind `:[key]="v"` — `props.rs:462-486`** (the `is_dynamic == Some(true)` block in `process_v_bind`).
Today one `out.overwrite(...&format!("{{...{{[{}]: {}}}}}", arg_resolved, value_resolved))`. Replace with: `OverwriteSyntheticBoundary("{...{[")` + **mapped** `arg` expression (`source_start = arg_expr_start`) + `OverwriteSyntheticBoundary("]: ")` + **mapped** value expression (`source_start = vs`) + `OverwriteSyntheticBoundary("}}}")`. Both identifiers map back; all punctuation maps to `None`.

**2E. Rewrite native v-model — `process_v_model` — `props.rs` (`fn` at `:826`)** (hardest, P2-A).
The current code embeds `resolved` up to 3× in one `format!` then `overwrite`s. Rewrite so **each** occurrence is a **separate `InsertMapped`** pointing at the SAME `source_start` (`vs..ve` trimmed); `occurrences` on `JsxBindingValue` records the count. Structure per branch:
- value binding: `InsertUnmapped("value={")` + `InsertMapped(expr@vs)` + `InsertUnmapped("} ")`.
- handler: `InsertUnmapped("onInput={($event:any) => ((")` + `InsertMapped(expr@vs)` + `InsertUnmapped(") = $event)}")`.
- dynamic-arg / component / named branches: same decomposition; computed-name/spread punctuation all `InsertUnmapped`; every embedded expression `InsertMapped@vs`; the dynamic **arg** expression maps to `raw_arg_start`.
- modifiers prop (`{}={{{{ {} }}}}`): each modifier name → `InsertMapped@m.start` (currently unmapped — upgraded so hovering a modifier resolves).
- Preserve all branch logic (has_explicit_prop / has_explicit_handler / both / dynamic / component); only the **emission mechanism** changes. Empty-replacement branches stay `overwrite(prop.start, prop_end, "")`.

**(P2-A) Deterministic reverse-occurrence selection.** Because one source span now maps to 2-3 generated occurrences, `vue_to_tsx` (Phase 1) must return a **deterministic** generated offset for a v-model source position. Rule: **prefer the first occurrence in generated order that is a READ position** (the value-binding occurrence) over an assignment-LHS occurrence. Implement this as the natural consequence of strict first-covering-run lookup in generated order (the value binding is emitted first), and pin it with a test (below). No heuristics — it is "first covering mapped run in generated byte order."

**2F. Migrate ALL IDE callers of the flat-string producers, then DELETE both (OQ-2, P2-C).**
`resolve_prefixed_expr` (`props.rs` `fn` at `:1279`) and `resolve_prefixed_dynamic_arg` (`props.rs` `fn` at `:1293`) return a flat `String` (prefix+ident). They are **IDE-only**. The **FULL** caller set (P2-C, enumerated — migrate every one to `EmitOp` before deleting):
- `resolve_prefixed_expr`: `process_element_props:279`, `process_v_bind` `:422`/`:436`/`:465`/`:505`, `process_v_on` `:594`/`:621`/`:647`/`:693`, `process_v_model:842`, `process_v_html:1032`, `process_v_text:1048`.
- `resolve_prefixed_dynamic_arg`: `process_v_bind:475`, `process_v_on:616`, `process_v_model:861`.
Each caller is rewritten to build a `JsxBindingValue` (using `oxc_prop.exp.bindings` + `resolver.resolve_prefix/suffix`) and call `emit_jsx_binding_value`. The already-correct split paths (static-key `:prop` `:504-564`, `.foo=` shorthand `:418-441`, `v-bind="obj"` spread `:444-456`) are migrated to `EmitOp` too (so the producers can be deleted cleanly) — `EmitOp` reproduces their exact split. **Delete both producers in the SAME Phase-2 commit** once no caller remains.

**SHARED helpers untouched (CRITICAL — do NOT modify):** `build_prefixed_expr` (`interpolation.rs:61-172`), `resolve_simple_expr`, `resolve_prefix`/`resolve_suffix`, `collect_binding_patches` (`binding.rs:297-342`). Their flat-string contract is depended on by the VDOM/Vapor runtime. Phase 2 changes only the **IDE** consumers' emission.

### Legacy Deletions
- `props.rs:1279` `resolve_prefixed_expr` — deleted after all 11 callers migrated.
- `props.rs:1293` `resolve_prefixed_dynamic_arg` — deleted after all 3 callers migrated.
- The four single-`overwrite(...format!...)` emissions at `process_v_html`, `process_v_text`, dynamic-key bind, native v-model (replaced by `EmitOp`/`emit_jsx_binding_value`).

### Tests (write FIRST; FAIL pre-change, PASS post-change)
Rust codegen tests in `crates/verter_compiler/src/ide/template/tests.rs` (compile a fixture with `CompileTarget::IDE`, build the source map, assert the **identifier maps to its source span** and prefix/punctuation map to `None`):
- `v_html_identifier_maps_to_source` — `<div v-html="msg"/>` → `msg` token maps back to the `msg` source offset; `innerHTML=`, `{`, `}` columns map to `None`.
- `v_text_identifier_maps_to_source` — symmetric for `<div v-text="content"/>`.
- `dynamic_key_bind_both_identifiers_map` — `<div :[key]="val"/>` → both `key` and `val` map back; `{...{[`, `]: `, `}}}` map to `None`.
- `native_vmodel_every_occurrence_maps_back` — **(architecture-named, negative-incl)** `<input v-model="count"/>` → enumerate ALL generated tokens equal/containing `count`; assert **each** maps back to the single `count` source span. Assert the assignment punctuation (`=>`, `($event`, `=`) maps to `None`.
- `vmodel_source_to_generated_selects_read_occurrence` — **(P2-A)** `vue_to_tsx` on the `count` source position returns the **value-binding** (read) generated occurrence, deterministically, not the assignment LHS. Discriminating: a non-deterministic / LHS-first selection fails.
- `vmodel_modifier_maps_to_source` — `<input v-model.trim="x"/>` → the `trim` modifier token maps to its source span.
- `vmodel_prefix_not_double_shifted` — **(P1-B)** the identifier after a `_ctx.`/`$setup.` prefix maps to the FIRST byte of the identifier (no double prefix shift, no unmapped identifier interior). Discriminating against the contradictory lowering.
- `synthetic_boundary_start_maps_to_none` — **(P1-C)** the column at the start of an `OverwriteSyntheticBoundary` (`innerHTML=` start) maps to `None`. Discriminating: a `Chunk::Overwritten` lowering would map it to the prop start.
- `vmodel_does_not_emit_single_overwritten_chunk` — assert the chunk list contains NO `Overwritten` chunk spanning both synthetic prefix and a user identifier (test-only chunk accessor, or assert via the map that prop-start does NOT carry the identifier's source mapping).
- `emit_codegen_crlf_and_tabs` — **(P2-B)** a CRLF, tab-indented fixture still maps identifiers exactly.

E2E (Phase 2 portion of `definition.test.ts`): the **Rust source-map** assertions above are the HARD gate and fully discriminate the compiler fix. The end-to-end CTRL+CLICK assertions for these constructs land in Phase 6/7 (LSP wired). The committed fixtures that contain `v-html`/`v-text`/`:[key]`/native `v-model` are added in this phase (under `packages/vue-vscode/e2e/fixtures/goto-definition/`) so Phase 6/7 e2e cannot skip (OQ-6).

### Architecture guards
- `crates/verter_compiler/tests/ide_no_baked_prefix_overwrite.rs` (new): `no_ide_codegen_bakes_prefix_into_mapped_overwrite` — source scan over `crates/verter_compiler/src/ide/template/**` asserting no `out.overwrite(` argument is a `format!` concatenating a binding prefix with a user-expression slice (forbid `out.overwrite\([^,]+,[^,]+,\s*&format!\(.*resolved`). Architecture-named: "no IDE codegen call passes `format!("{}{}", prefix, ident)` into a mapped overwrite."
- `RETIRED_IDE_EMIT_SYMBOLS` (in the same test): assert `resolve_prefixed_expr` and `resolve_prefixed_dynamic_arg` are absent from `crates/verter_compiler/src/**` (P2-C, RETIRED_SYMBOLS-style name guard so a lingering producer fails CI).
- `emit_op_has_no_mapped_overwrite_variant` (unit, in `emit.rs`): enumerate `EmitOp` variants and assert none carries both a synthetic prefix and a `source_start` on the same overwrite (the bug is unrepresentable by the type).

### Verification
```
cargo nextest run --workspace 2>&1 | tee /tmp/p2.txt
cargo test -p verter_session --tests 2>&1 | tee -a /tmp/p2.txt
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
pnpm build:native   # IDE codegen feeds the LSP/e2e
```
Expected: all Rust codegen+mapper tests green; the four constructs produce maps where the user identifier(s) map back exactly and synthetic text maps to `None`. VDOM/Vapor runtime codegen output **byte-identical** to pre-change (existing `template/code_gen` snapshot/parity tests pass unchanged — proves the shared helpers were untouched).

---

## PHASE 3 — `SfcComponentAnchor` as first-class analysis output at `IndexedReady`; delete `find_export_span` heuristic

### Context (OQ-3 BINDING)
`.vue` default imports/tags must resolve to a **component anchor**, not the first script binding. Today `find_export_span` (`fn` at `crates/verter_session/src/host_manage/analysis_io.rs:1920`) returns, for `binding_name == "default"` on a Vue SFC, the first binding → first macro → `(0,0)` (the `bindings.first()`/`macros.first()` heuristic body at `:1944-1954` ending in `return Some((0, 0))` at `:1954`). OQ-3 binds: compute `SfcComponentAnchor` at **Vue `IndexedReady` publication** (script + template guaranteed ready), store it **NON-OPTIONAL** via a new `TemplateAnalysisState`, add a producer-side `defineOptions({name})` name-span field for priority-1, and discriminate priority-4 (template-only → `TemplateRootStart`, never silent `FileStart`).

### Changes

**3A. New producer field for the `defineOptions({ name })` name span (P1-F).**
`AnalyzedMacro` (`crates/verter_semantic/src/analysis/types.rs:1306-1365`) has `kind`, `prop_fields`, whole-macro `span` (`:1363`) but **no** name-prop value span. Add:
```rust
// on AnalyzedMacro (populated only for AnalyzedMacroKind::DefineOptions)
pub define_options_name_span: Option<verter_span::Span>,
```
Capture it in shallow analysis where `defineOptions` props are walked (the producer that fills `prop_fields` for `DefineOptions`): when a `name:` property with a string-literal value is present, record the **value literal span** (SFC-absolute). This is the only data needed for priority-1; without it, priority-1 is unimplementable (it would fall back to whole-macro span). TDD: the `anchor_prefers_define_options_name` test (below) fails until this field exists and is populated.

**3B. New `SfcComponentAnchor` + `SfcAnchorKind` + `TemplateAnalysisState` (OQ-3, placement per OQ-5).**
Per OQ-5 the nav core is LSP-free in `verter_session::navigation::definition`; `SfcComponentAnchor`/`SfcAnchorKind` live there (re-exported as needed) and are serde-able for storage on the analysis record:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SfcComponentAnchor { pub preferred_span: verter_span::Span, pub kind: SfcAnchorKind }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SfcAnchorKind {
    DefineOptionsName,     // 1
    ExplicitExportDefault, // 2
    ScriptSetupStart,      // 3
    TemplateRootStart,     // 4
    FileStart,             // 5 — ONLY for truly empty SFCs, recorded explicitly
}
```
And the **non-optional** template state (OQ-3) on the Vue analysis record, replacing the `Option<Arc<TemplateAnalysisSnapshot>>` on the Vue path:
```rust
pub enum TemplateAnalysisState {
    NoTemplate,                                   // SFC has no <template>
    Ready(Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>),
}
```
`FileAnalysisSnapshot.template` stays `Option` at the TS boundary only (`sfcComponentAnchor?` / non-SFC has none), but the **Vue** record carries `TemplateAnalysisState` so priority-4 cannot silently degrade. **A `.vue` nav artifact must NOT publish `IndexedReady` if template analysis is deferred** — i.e. `IndexedReady` for a `.vue` always carries a resolved `TemplateAnalysisState` (either `NoTemplate` or `Ready`). The template-root span itself is computed from the **SFC block syntax** (the parsed `<template>` open-tag start in `cached_parse: Option<Arc<ParsedSfc>>`), which is available independent of semantic template analysis, guaranteeing priority-4 even when deeper template semantics are deferred.

**3C. Compute the anchor at `IndexedReady` publication (OQ-3 production site PINNED).**
Add `pub sfc_component_anchor: Option<SfcComponentAnchor>` to `IndexedReady` (`crates/verter_session/src/project_type_store.rs:95-164` — struct + doc; `whole_hash` first field at `:115`) — `Some` for every `.vue` record, `None` for non-SFC. Populate it at the two `IndexedReady` build sites:
- `crates/verter_session/src/host_manage/prepared_decl.rs:1708` (`ensure_indexed_ready`), and
- `crates/verter_session/src/host_manage/overlay_materialize.rs:533` (overlay materialization),
by calling a new `fn compute_sfc_component_anchor(script: &ScriptAnalysisSnapshot, template: &TemplateAnalysisState, parsed: Option<&ParsedSfc>) -> SfcComponentAnchor` applying the **fixed priority 1→5** exactly:
1. `defineOptions({ name })` name span (from `AnalyzedMacro.define_options_name_span`).
2. explicit `<script> export default` expr/object span (from `script_analysis` options-api / `export_signatures`).
3. `<script setup>` tag start (from `ParsedSfc` script-setup block start).
4. first `<template>` root tag start (from `ParsedSfc` template block / `TemplateAnalysisState::Ready` root tag open start).
5. `FileStart` — only when the SFC is truly empty (no script, no template, no default export), recorded explicitly.
All inputs are available at `IndexedReady` build time (script analysis + `cached_parse` + `TemplateAnalysisState`). The anchor is mirrored onto `FileAnalysisSnapshot` (TS `sfcComponentAnchor?` in `packages/language-shared/src/analysis.ts:435-449`) for any TS consumer.

**3D. Rewrite `find_export_span` to consume the anchor.** Replace the `binding_name == "default"` Vue branch heuristic body (`analysis_io.rs:1944-1954`) so a Vue SFC default export returns `Some((anchor.preferred_span.start, anchor.preferred_span.end))` from the recorded `SfcComponentAnchor` (read from `IndexedReady`). **Delete** the first-binding → first-macro → `(0,0)` cascade entirely. Non-default Vue bindings keep the existing binding/macro span lookup; non-SFC files keep `export_signatures`.

### Legacy Deletions
- `analysis_io.rs:1944-1954` — the `binding_name == "default"` first-binding/first-macro/`(0,0)` heuristic (the three fallthroughs ending in `return Some((0, 0))` at `:1954`).
- The `Option<Arc<TemplateAnalysisSnapshot>>` field on the **Vue** analysis record where it permitted a deferred template (replaced by the non-optional `TemplateAnalysisState` for the Vue path).
- Any other path synthesizing a `.vue` default target as the first binding (grep `bindings.first()` / `macros.first()` in nav/definition code; confirm none remain for the default-export case).

### Tests (write FIRST; FAIL pre-change, PASS post-change)
Rust unit tests (new `crates/verter_session/src/host_manage/sfc_anchor_tests.rs`) using `VerterHost::new_standalone` + `upsert` (pattern from `vue_sfc_absolute_spans.rs`):
- `anchor_prefers_define_options_name` — SFC with `defineOptions({ name: 'Foo' })` → anchor span slices to `Foo` (priority 1). **Fails until 3A lands.**
- `anchor_explicit_export_default` — `<script>export default { ... }</script>` (no defineOptions) → anchor covers the export-default object (priority 2).
- `anchor_script_setup_start` — `<script setup>` only → anchor at `<script setup>` tag start (priority 3).
- `anchor_template_root_start` — **(P1-G discriminating)** template-only SFC → anchor at first template root tag start (priority 4), **NOT FileStart**. Fails if priority-4 degrades to FileStart.
- `anchor_file_start_only_for_empty` — truly empty SFC → `FileStart` (priority 5).
- `find_export_span_default_is_anchor_not_first_binding` — **discriminating**: SFC with a leading `const helper = 1` before `defineOptions({name:'Foo'})`; `find_export_span(.., "default")` returns the `Foo` span, **not** the `helper` span. FAILS on the pre-change tree.
- `find_export_span_default_never_returns_zero_zero` — never `(0,0)` for a non-empty SFC.
- `vue_indexed_ready_carries_resolved_template_state` — **(OQ-3)** a `.vue` `IndexedReady` always carries `TemplateAnalysisState != deferred` (either `NoTemplate` or `Ready`).

### Architecture guards
- `crates/verter_session/tests/architecture_guards.rs` `every_vue_record_has_anchor`: build a host with a representative `.vue` and assert `IndexedReady.sfc_component_anchor.is_some()`; assert a non-SFC `.ts` yields `None`.
- `vue_record_template_state_never_deferred` (discriminating, OQ-3): assert a `.vue` `IndexedReady`'s `TemplateAnalysisState` is `NoTemplate` or `Ready` (never absent), AND that a template-only SFC's anchor kind is `TemplateRootStart` not `FileStart` (P1-G).
- `ban_first_binding_default_export_heuristic` (source scan over `analysis_io.rs`): assert the deleted `return Some((0, 0))` and `bindings.first()`/`macros.first()` default-export fallbacks are absent.

### Verification
```
cargo nextest run --workspace 2>&1 | tee /tmp/p3.txt
cargo test -p verter_session --tests 2>&1 | tee -a /tmp/p3.txt
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
```
Expected: green. `.vue` default-export span resolution points at the component anchor regardless of leading helpers; template-only SFCs anchor at the template root.

---

## PHASE 4 — `CompileSnapshotId` (compile warm-hit fact-validated identity) + host-sourced target-mapper / source-context loading

### Context (OQ-4 BINDING — reconciled to the unified §3.1.2 authority)
Cross-file targets must be mapped using **the target file's own** source map, loaded from the **host** (not the open-doc registry), valid only when its snapshot matches the TSX snapshot tsgo resolved against. Today target mappers come from `get_position_mapper` (open documents only) so a closed target `.vue` cannot be mapped (→ `Range::default()`), and there is no snapshot identity. The validity token is `CompileSnapshotId`, the **compile warm-hit fact-validated identity** the unified plan §3.1.2 reconciled to (see 4A) — it tracks both own-content and cross-file dependency edits, so a stale mapper is caught even when the generated TSX bytes are unchanged.

### Changes

**4A. New `CompileSnapshotId(u128)` (OQ-4) — `crates/verter_session/src/navigation/snapshot.rs` (or `types.rs`).**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] // Copy + Send + Sync
pub struct CompileSnapshotId(pub u128);
```
**OQ-4 derivation (resolves P0-2; reconciled to the unified §3.1.2 authority — this WINS over the round-1 TSX-byte-hash sketch):** `CompileSnapshotId = Hash128(compile warm-hit identity)` = `semantic_hash` + the style/content override hashes + the compile profile + the parser/compiler/version dimensions + the **canonicalized `ReadSetSignature` facts** (the cross-file fact signature). This is the EXACT fact-validated identity a compile warm hit is admitted under — the same key `compile_slot_facts_validate` decides validity by. It is computed **once at compile-cache publish** (at `virtual_file_pipeline.rs:1851` where `CachedTsx` is built) and **stored**; warm hits return the **stored** id (O(1) `u128` compare; no per-request recompute). Why the fact signature is load-bearing: `semantic_hash` covers **own-content only**, so a bare TSX-byte hash is **stale-stable under cross-file dependency edits** — a dep edit that changes a target's meaning but not its emitted TSX bytes would keep an old byte-hash id and serve a stale mapper. Folding the canonicalized `ReadSetSignature` facts into the id closes that hazard: a cross-file fact-signature change invalidates `CompileSnapshotId` even when the generated TSX bytes are byte-for-byte identical.

A `Hash128` over the generated TSX bytes (the round-1 sketch, `Hash128("verter.ide.tsx.v1" || generated_tsx_bytes)`) MAY ride alongside as **optional debug / collision metadata only** — it is NOT the validity token and is never compared to decide whether a mapper is fresh.

**4B. Store `CompileSnapshotId` at the publish point.**
- Add `pub snapshot_id: CompileSnapshotId` to `CachedTsx` (`types.rs:1723-1727`) — populated at construction (`virtual_file_pipeline.rs:1851`) from the compile warm-hit identity the slot was admitted under (the `semantic_hash` + override hashes + profile + parser/compiler/version dimensions + canonicalized `ReadSetSignature` facts already available at publish; see 4A). It flows through `CompileOutputValue.tsx: Option<CachedTsx>` (`cache_runtime/compile_output_node.rs:130`) for free.
- Add `pub snapshot_id: CompileSnapshotId` to `IdeResponse` (`types.rs:1732-1741`) and populate it in `get_ide` (the `IdeResponse` literal at `virtual_file_pipeline.rs:1366-1371`) from `CachedTsx.snapshot_id` (no recompute — read the stored id, available via `CompileOutputNodeFactValidatedSession::peek_tsx`).
- Store the `CompileSnapshotId` on the **tsgo synced-file registry** (`crates/verter_lsp/src/tsgo/project_sync.rs`) when a `.vue.tsx` is synced to tsgo, so a returned `TsDefinitionSpan` can be tagged with the snapshot it answered against (OQ-1 / OQ-4).

**4C. New host-sourced target contexts (OQ-4: TSX targets AND non-TSX targets) — `virtual_file_pipeline.rs`.**
```rust
pub struct TargetIdeContext {                 // for .vue targets (have generated TSX)
    // (P1-A) carry BOTH canonicals explicitly: the owner .vue source (the
    // normalize/render/dedup key) and the generated .vue.tsx registry key.
    pub source_canonical: CanonicalId,        // = Arc<str>; the owner .vue source canonical
    pub generated_canonical: CanonicalId,     // = Arc<str>; the generated .vue.tsx registry key
    pub tsx_code: Arc<str>,
    pub source_map_json: Option<Arc<str>>,
    pub snapshot_id: CompileSnapshotId,
    pub is_jsx: bool,
}
pub struct TargetSourceContext {              // for .ts/.js/.d.ts targets (no TSX)
    pub canonical_id: CanonicalId,
    pub source: Arc<str>,
    pub line_index: Arc<LineIndex>,
    pub source_hash: Hash16,                  // validates the source revision used
}
impl VerterHost {
    /// Ensure compiled, then return the .vue target's IDE context (code + map + snapshot),
    /// sourced entirely from the host — independent of the open-document registry.
    /// **(P1-A)** `source_canonical` is the OWNER `.vue` source canonical (the key);
    /// the returned context exposes BOTH `source_canonical` and `generated_canonical`.
    pub fn target_ide_context(&self, source_canonical: &str, profile: &CompileProfile) -> Option<TargetIdeContext>;
    /// Host source context for a non-TSX target (.ts/.js/.d.ts): source + LineIndex + source hash.
    pub fn target_source_context(&self, canonical_id: &str) -> Option<TargetSourceContext>;
    /// **(P1-A)** Map a generated `.vue.tsx`/`.vue.jsx` path back to its OWNER `.vue`
    /// source canonical (path-suffix strip + host source-existence guard). Sole owner
    /// of the generated→owner derivation; the `TsgoNavigationBackend` adapter calls
    /// this when constructing `GeneratedTsx`. Returns `None` if the stripped `.vue`
    /// is not a host source (so a real on-disk `.vue.tsx` is never mis-stripped).
    pub fn owner_canonical_for_generated(&self, generated: &str) -> Option<CanonicalId>;
}
```
`target_ide_context`: takes the OWNER `.vue` `source_canonical`; `self.ensure_compiled(source_canonical, profile).ok()?;` (re-validating cross-file facts) then `let ide = self.get_ide(source_canonical, profile)?;` and assemble `TargetIdeContext { source_canonical, generated_canonical, .., snapshot_id: ide.snapshot_id, .. }` — `generated_canonical` is the host-derived `.vue.tsx` virtual key for `source_canonical` (the forward derivation already owned by `virtual_file_pipeline.rs`). **(P1-A)** `owner_canonical_for_generated` is the inverse (path-suffix strip + `get_source` existence guard) — the single owner of the generated→owner mapping the adapter uses. `target_source_context`: read the host source + build/cached `LineIndex` + `source_hash` from `IndexedReady`/`FileArtifactStore`. **(P0-2)** `.ts/.js/.d.ts` real-source targets have NO IDE TSX → they validate via `TargetSourceContext.source_hash`, NOT a TSX `CompileSnapshotId`; `CompileSnapshotId` is **only** for generated→Vue mapper validation.

**4D. LSP-side host-sourced resolver — `crates/verter_lsp/src/server/provider_state.rs`.**
Add `fn host_target_context(&self, canonical_id: &str) -> Option<HostTargetContext>` and `fn host_source_context(&self, canonical_id: &str) -> Option<HostSourceContext>` that call the two host APIs, build a `LineIndex` + `PositionMapper` (TSX case) or just a `LineIndex` (source case), and carry the `CompileSnapshotId`/`source_hash`. These will **replace** `external_ide_context` (`provider_state.rs:26-40`) and `ide_context_by_path` (`sync_orchestration.rs:1380-1389`) for cross-file targets — but the **deletion** of the old readers happens in Phase 6/7 (their last callers — the merge functions — are deleted there). Phase 4 ships the new APIs **dormant-but-tested**; no live nav handler calls them yet, so there is **no second active path** (the old readers remain the only ones the handlers call until Phase 5/6-7 flips the consumer in one move). This is additive infrastructure, not a parallel resolver.

### Legacy Deletions
- None in Phase 4. (The open-doc cross-file readers `external_ide_context` / `ide_context_by_path` are deleted in Phase 6/7 when their last callers — the merge functions — are removed. Phase 4 adds infra only, to keep this a no-dual-path landing.)

### Tests (write FIRST; FAIL pre-change, PASS post-change)
Rust integration tests in `crates/verter_session` (`VerterHost::new_standalone` + upsert; target NEVER "opened"):
- `target_ide_context_for_unopened_file` — upsert `Child.vue`; `target_ide_context("…/Child.vue", &profile)` → `Some` with non-empty `tsx_code` and a `snapshot_id`. (No open-doc registry.)
- `target_ide_context_carries_both_canonicals` — **(P1-A)** the returned `TargetIdeContext.source_canonical` is the `.vue` source canonical and `.generated_canonical` ends `.vue.tsx`; they are distinct. Discriminating: collapsing to a single `canonical_id` (the old shape) fails.
- `owner_canonical_for_generated_strips_suffix` — **(P1-A, discriminating)** `owner_canonical_for_generated("…/Child.vue.tsx")` → `Some("…/Child.vue")` for a `.vue` present in the host; `owner_canonical_for_generated("…/util.ts")` (not a generated `.vue.*`) → `None`; and a `.vue.tsx` whose stripped `.vue` is NOT a host source → `None` (no mis-strip of a real on-disk `.vue.tsx`). FAILS if the derivation is absent or unguarded.
- `snapshot_id_stable_across_reads` — two `target_ide_context` calls without an edit → equal `CompileSnapshotId`.
- `snapshot_id_changes_on_recompile` — edit `Child.vue` (own content that changes the compile identity) → `CompileSnapshotId` differs.
- `snapshot_id_invalidated_by_cross_file_fact_change` — **(OQ-4/P0-2 discriminating; reconciled to §3.1.2)** edit a CROSS-FILE dependency in a way that changes the owner's canonicalized `ReadSetSignature` facts **without changing the owner's generated TSX bytes** → `CompileSnapshotId` MUST differ. Discriminating: a bare TSX-byte-hash id would keep the same value here (stale-stable) and FAIL; the fact-validated identity catches it. The paired same-content case (a recompile producing the identical compile identity) keeps the id equal.
- `get_ide_response_carries_snapshot` — `get_ide(...).snapshot_id == target_ide_context(...).snapshot_id` (== `CachedTsx.snapshot_id`; all read the stored id).
- `target_source_context_for_ts_file` — **(P0-2)** `target_source_context("…/util.ts")` → `Some` with `source`, `line_index`, `source_hash`; no TSX/`CompileSnapshotId` involved.
- LSP-side `host_target_context_builds_mapper_for_unopened_target` — a `PositionMapper` is produced for a target not in `documents`.

### Architecture guards
- `crates/verter_session/tests/architecture_guards.rs` `target_mapper_is_host_sourced`: `target_ide_context` succeeds for a file present in the host but absent from any open-doc registry (fails if target mapping regresses to open-doc-only).
- `snapshot_id_is_fact_validated_identity` (unit): assert `CompileSnapshotId` equals the compile warm-hit identity hash (`semantic_hash` + override hashes + profile + parser/compiler/version + canonicalized `ReadSetSignature` facts) for a known compile — and assert it is NOT equal to a bare `Hash128(prefix || generated_tsx_bytes)` of the same TSX when the fact signature differs (pins the §3.1.2 reconciliation; a TSX-byte-hash re-derivation that ignores the cross-file fact signature fails). The optional debug/collision TSX-byte hash, if carried, is asserted to be metadata-only (never the comparison the freshness check uses).
- `ban_open_doc_mapper_for_cross_file_targets` (source scan; **registered now, fully enforced in Phase 6/7**): the cross-file target path does not call `documents.get_position_mapper` / `get(...).position_mapper`.

### Verification
```
cargo nextest run --workspace 2>&1 | tee /tmp/p4.txt
cargo test -p verter_session --tests 2>&1 | tee -a /tmp/p4.txt
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
```
Expected: green; new host API exercised; live behavior unchanged (APIs dormant until Phase 5).

---

## PHASE 5 — `verter_session::navigation::definition` core + `TsNavigationBackend`; route definition through the engine

### Context (OQ-1 + OQ-5 BINDING)
Collapse the three-path design into one engine. Per the architecture: `LSP position → classify into DefinitionQuery → map into generated TSX if needed → ask the location-only nav backend when semantic TS resolution is required → normalize results into DefinitionTarget → terminalize barrels and Vue default exports → render the target into an LSP Location using the target file's own snapshot → exact dedup.`

**OQ-5 (placement) — P1-3:** the canonical nav core is **LSP-FREE** in `verter_session::navigation::definition` — `DefinitionRequest`, `DefinitionTarget`, `DefinitionQuery`, `DefinitionEngine`, `DefinitionSymbolKind`, `CompileSnapshotId`, `TargetIdeContext`/`TargetSourceContext`, `SfcComponentAnchor`/`SfcAnchorKind`, dedup identity, target normalization, barrel terminalization, host-sourced mapper lookup, **`DefinitionQuery` classification, AND the Vue→TSX position mapping** (`DefinitionEngine` owns BOTH `classify` and the active-file Vue→generated-TSX map — both run inside the engine against host-sourced mappers; see 5C). **No `tower_lsp`/`lsp_types` in `verter_session`.** `verter_lsp` does **only three** things: (1) convert `tower_lsp::Position` → the typed request coordinates (`LspPosition`) of the `DefinitionRequest`, (2) implement and inject the `TsgoNavigationBackend`, and (3) render the returned `DefinitionTarget` → `tower_lsp::Location` via the target `LineIndex`/source-map/snapshot check. `verter_lsp` does NOT classify the query and does NOT map Vue→TSX — those moved into the engine (P1-3).

**OQ-1 (one engine):** semantic-TS declaration lookup goes through a **location-only** `TsNavigationBackend` returning an **opaque** `TsDefinitionSpan` — never typed-IR. (Type/member-valued answers, needed only by type-definition in Phase 6/7, route through `ProjectSemanticDispatch`.)

### Changes

**5A. New `TsNavigationBackend` trait (OQ-1) — `verter_session::navigation` (trait); impl in `verter_lsp::tsgo`.**
**(P1-1 typed coordinate-space + provenance.)** A backend answer is modelled by its **coordinate space AND its provenance token together** — never a span in one space with a stale-stability token from the other. `GeneratedByteRange` (new, Phase 1 — see 1A) pairs with `CompileSnapshotId`; `SourceByteRange` (real source) pairs with `Hash16` (`= [u8;16]`, the real workspace type at `crates/verter_session/src/types.rs:12`). The location-only result is therefore a provenance-bearing enum, NOT a struct with a bare `Option<CompileSnapshotId>`:
```rust
// verter_session::navigation
pub enum TsDefinitionSpan {
    /// tsgo resolved into a generated `.vue.tsx`; the span is a GENERATED byte
    /// range and is only meaningful while the snapshot matches.
    /// **(P1-A)** Carries BOTH the generated `.vue.tsx` registry key and the
    /// OWNER `.vue` source canonical explicitly — normalize/render/dedup all
    /// key on `owner_canonical`, while `generated_canonical` is only used to
    /// re-fetch the target's host-sourced IDE context (TSX + map) for the
    /// generated→source mapping. The adapter derives `owner_canonical` at
    /// construction (it is the only place that sees the raw tsgo `.vue.tsx`
    /// path) via the verified path-suffix-strip mechanism (see 5D).
    GeneratedTsx {
        owner_canonical: CanonicalId,     // = Arc<str>; the OWNER .vue source canonical (normalize/render/dedup key)
        generated_canonical: CanonicalId, // = Arc<str>; the generated .vue.tsx registry key (for re-fetching IDE ctx)
        span: GeneratedByteRange,         // byte range in generated TSX (NOT SourceByteRange)
        snapshot_id: CompileSnapshotId,          // the TSX snapshot tsgo answered against
    },
    /// tsgo resolved into a real host file (.ts/.js/.d.ts); the span is a
    /// REAL-SOURCE byte range validated by the file's source hash.
    HostSource {
        canonical_id: CanonicalId,
        span: SourceByteRange,            // byte range in real source
        source_hash: Hash16,              // = [u8;16]; the source revision answered against
    },
}
/// An opaque, provenance-bearing rename edit target (location-only): WHERE a
/// rename must edit, plus the replacement text. No type payload.
pub struct TsRenameEdit { pub location: TsDefinitionSpan, pub new_text: String }
/// An opaque, provenance-bearing code-action navigable edit (location-only):
/// WHERE an action's edit applies, plus the text. No type payload, no quickinfo.
pub struct TsCodeActionEdit { pub title: String, pub kind: Option<String>, pub edits: Vec<TsRenameEdit> }
#[async_trait] pub trait TsNavigationBackend {
    async fn definition_at_position(
        &self, tsx_canonical: &str, offset: GeneratedByteOffset, snapshot: CompileSnapshotId,
    ) -> Vec<TsDefinitionSpan>;
    /// Reference SITES only — each an opaque provenance-bearing location.
    async fn references_at_position(
        &self, tsx_canonical: &str, offset: GeneratedByteOffset, snapshot: CompileSnapshotId,
    ) -> Vec<TsDefinitionSpan>;
    /// Rename edit TARGETS only — opaque location + replacement text.
    async fn rename_at_position(
        &self, tsx_canonical: &str, offset: GeneratedByteOffset, snapshot: CompileSnapshotId,
    ) -> Vec<TsRenameEdit>;
    /// Code actions whose edits carry navigable TARGETS only — opaque
    /// locations + text; never type-valued payloads/quickinfo.
    async fn code_actions_at_position(
        &self, tsx_canonical: &str, range: GeneratedByteRange, snapshot: CompileSnapshotId,
    ) -> Vec<TsCodeActionEdit>;
}
```
**(P1-2.)** All four methods return **location-only, provenance-bearing** values (`TsDefinitionSpan`/`TsRenameEdit`/`TsCodeActionEdit`) — never type payloads — so Phase 6/7-C can route references/rename/code-action through this backend (the guard `direct_tsgo_nav_only_in_adapter` bans direct tsgo nav elsewhere; without these siblings that guard would be unsatisfiable, because 6/7-C needs a non-tsgo nav surface for refs/rename/code-action). `TsgoNavigationBackend` implements all four over the EXISTING `TypeProvider` methods (verified): `get_references -> Vec<TypeLocation>` (`extension_provider.rs:434`), `get_rename_locations -> Vec<RenameLocation>` (`:474`), `get_code_actions -> Vec<TypeCodeAction>` (`:657`) — each `TypeLocation`/edit is wrapped into a `TsDefinitionSpan` (`GeneratedTsx`/`HostSource`) exactly as `definition_at_position` does, tagging the provenance token at construction. (Type-definition is NOT a method here — per OQ-7 it routes its type computation through `ProjectSemanticDispatch`, then renders via the shared core; it does not use this location-only backend.)
**BOUNDARY (guarded):** `TsDefinitionSpan`/`TsRenameEdit`/`TsCodeActionEdit` expose NO `TypeExpr`, `SemanticNodeId`, checker types, member lists, or quickinfo — a location (+ replacement text) only. `verter_lsp::tsgo::TsgoNavigationBackend` implements it over `type_provider.get_definition` (`getDefinitionAtPosition`): each returned `TypeLocation` whose path is a `.vue.tsx`/`.vue.jsx` becomes `GeneratedTsx { owner_canonical, generated_canonical, span: GeneratedByteRange, snapshot_id }` — the adapter derives `owner_canonical` from the raw tsgo `.vue.tsx` path **(P1-A)** via the verified path-suffix-strip mechanism (`.vue.tsx`/`.vue.jsx` → `.vue` by trimming the 4-char virtual suffix), guarded by a host source-existence check (the existing `vue_source_exists` predicate is `|p| host.get_source(p).is_some()`), so a real on-disk `.vue.tsx` is never mis-stripped; `generated_canonical` keeps the raw `.vue.tsx` key for re-fetching the target's IDE context; the span is tagged with the synced-file `CompileSnapshotId` from the tsgo registry (Phase 4B). The adapter is the ONLY place that sees the raw tsgo `.vue.tsx` path, so it is the correct (and only) site for the generated→owner canonical derivation. Each returned `TypeLocation` whose path is a real host file becomes `HostSource { span: SourceByteRange, source_hash }` tagged with the target's `target_source_context(...).source_hash` (Phase 4). The coordinate space and the provenance token are chosen together at construction — there is no representable state where a generated span carries a `source_hash` or a real-source span carries a `CompileSnapshotId`.

**(P1-A) Verified virtual→source canonical mechanism (named).** There is **no** reverse `.vue.tsx`→`.vue` registry in the host. The forward direction is host-owned: `virtual_file_pipeline.rs` derives a `.vue`'s virtual TSX/component name from the `.vue` canonical (`:1440`) and enumerates a `.vue` canonical's virtual nodes via `list_virtual_files`/`list_virtual_nodes` (`:1346`). The **reverse** mapping a generated target needs is **path-suffix stripping**, currently implemented by `normalize_vue_path` (`fn` at `crates/verter_lsp/src/tsgo/merge.rs:829`) / `normalize_vue_path_owned` (`fn` at `:849`) — strip `.vue.tsx`/`.vue.jsx` (trim 4 chars), `.vue.ts` (trim 3), `.vue.d.ts` (trim `.d.ts`) — guarded by `vue_source_exists` (`nav_features.rs:807-808` = `|p| host.get_source(p).is_some()`). Because `merge.rs` is **deleted** in §6/7 and OQ-5 requires the nav core be LSP-free, this strip+existence-check logic moves to a small host/engine helper (e.g. `VerterHost::owner_canonical_for_generated(generated: &str) -> Option<CanonicalId>`) that the `TsgoNavigationBackend` adapter calls when constructing `GeneratedTsx`. The helper is the single owner of the generated→owner derivation; no nav code re-implements path stripping (covered by the existing `ban_suffix_based_dedup` / nav source-scan guards plus the P1-A normalization test below).

**5B. New nav-core types — `verter_session::navigation::definition` (OQ-5, LSP-free).**
**(P1-1 coordinate-space + provenance.)** Every `DefinitionTarget` span is in the **target file's own source coordinates** (`SourceByteRange`), but each variant carries the **provenance token that validated how that span was obtained**, modelled explicitly so a `.vue`-via-TSX span (snapshot-validated) can never be confused with a `.ts/.js/.d.ts` real-source span (source-hash-validated):
```rust
pub struct DefinitionRequest { pub canonical_id: CanonicalId, pub position: LspPosition, /* … */ }
/// How a target's source span was validated. Explicit so the renderer
/// validates the RIGHT token (P1-1): a generated→source mapping is only
/// valid while the TSX snapshot matches; a real-source span is only valid
/// while the file's source hash matches; a same-file live span needs no
/// remote token.
pub enum TargetProvenance {
    GeneratedMapping(CompileSnapshotId), // span produced by mapping a generated TSX range → .vue source; validate CompileSnapshotId
    HostSource(Hash16),           // span is a real-source byte range (.ts/.js/.d.ts/named .vue); validate source_hash (= [u8;16])
    LiveSameFile,                 // same-file binding resolved from live analysis; no remote validation
}
pub enum DefinitionTarget {
    RealSource { uri: CanonicalId, span: SourceByteRange, symbol: DefinitionSymbolKind, provenance: TargetProvenance },
    SfcComponent { uri: CanonicalId, anchor: SfcComponentAnchor, provenance: TargetProvenance },
    ExternalDeclaration { uri: CanonicalId, span: SourceByteRange, symbol: DefinitionSymbolKind, source_hash: Hash16 },
}
pub enum DefinitionQuery {
    SameFileBinding { /* … */ },          // resolved from verter analysis
    CrossFileImport { canonical_id: CanonicalId, binding_name: String },
    ComponentTag { canonical_id: CanonicalId }, // .vue or component
    TemplateSemantic { tsx_offset: GeneratedByteOffset }, // needs the nav backend
    BarrelExport { /* … */ },
    // (P1-B) same-file CSS/DOM navigation over host-owned analysis data
    // (FileAnalysisSnapshot template/styles/dom_query_calls) — engine-owned,
    // LSP-free, never tsgo / typed-IR; each resolves to a same-file
    // DefinitionTarget::RealSource { provenance: LiveSameFile }.
    CssSelectorFromTemplate { /* class/id target span in a template attr */ },
    CssSelectorFromStyle { /* selector target span in a <style> block */ },
    DomQuerySelector { /* selector literal span in a querySelector(...) call */ },
}
pub enum DefinitionSymbolKind { /* LSP-free symbol kind enum */ }
```
**(P0-2 / OQ-4 / P1-1)** `RealSource`/`SfcComponent` carry `provenance: TargetProvenance`: `GeneratedMapping(CompileSnapshotId)` when the `.vue` source span was produced by mapping a generated-TSX range back (the only case needing a TSX snapshot), or `LiveSameFile` for a same-file binding resolved from live analysis (no remote token). A `.ts/.js` target whose real-source span tsgo returned directly is `RealSource` with `provenance = HostSource(Hash16)`. `ExternalDeclaration` (library `.d.ts`/`.vue.d.ts`) carries `source_hash: Hash16` directly, NOT a `CompileSnapshotId`. **(P1-I)** A target is renderable only if its provenance token still matches at render time (`GeneratedMapping` → `target_ide_context(uri).snapshot_id == CompileSnapshotId`; `HostSource`/`ExternalDeclaration` → `target_source_context(uri).source_hash == Hash16`; `LiveSameFile` → always renderable); otherwise it is **DROPPED** (never recomputed-in-place-and-remapped).

**5C. New `DefinitionEngine` pipeline — `verter_session::navigation::definition` (host-backed, LSP-free).**
- `classify(request, analysis, blocks) -> Vec<DefinitionQuery>` — folds in the classification logic currently in `definition_at_position` (`features/definition.rs` `fn` at `:41`), `try_component_contract_definition` (`server/component_resolve.rs` `fn` at `:375`), `try_barrel_export_definition` (`server/component_resolve.rs` `fn` at `:584`). This is a **move** into the engine, not a fork. **(P1-B) The CSS-selector and DOM-query leaf resolvers ALSO move into the engine** — they are pure SAME-FILE navigation over host-owned analysis data (verified: `css_definition_from_template`/`find_css_target_in_template`/`find_css_selector_definition`/`css_definition_from_style`/`find_css_target_in_style`/`dom_query_definition`/`dom_query_css_fallback` in `features/definition.rs:479-…` consume ONLY `FileAnalysisSnapshot` (`.template`/`.styles`/`.dom_query_calls`), the raw SFC `source`, and a `LineIndex`, and emit a same-file `span_definition` — they make NO tsgo call and NO typed-IR dispatch), so they belong in the LSP-free engine per OQ-5, NOT in the LSP driver (see 5E).
- `TemplateSemantic` queries **(P1-3 / OQ-5: mapping + classification live in the engine, NOT the LSP driver):** `classify` (above) decides a position is a `TemplateSemantic` query, and the **engine** maps the active file's Vue `LspPosition` → `GeneratedByteOffset` using the **active file's own host-sourced mapper** (`target_ide_context(active_canonical)` → `PositionMapper`, Phase 4 — NOT the open-doc registry, NOT the LSP driver). The engine then calls the injected `TsNavigationBackend::definition_at_position(tsx_canonical, GeneratedByteOffset, snapshot)` and normalizes each `TsDefinitionSpan` into a `DefinitionTarget` (5D). **(OQ-1)** The engine itself NEVER issues a `SemanticQueryKey`. The `TsNavigationBackend` is the ONLY thing `verter_lsp` injects; the Vue→TSX map and the `DefinitionQuery` classification are owned by the engine.
- same-file / import / component-tag / barrel queries: resolve from verter analysis + export graph (`get_export_span_follow_reexports`) + the `SfcComponentAnchor` (Phase 3), producing `DefinitionTarget`s directly.
- **(P1-B)** `CssSelectorFromTemplate` / `CssSelectorFromStyle` / `DomQuerySelector` queries: resolved **inside the engine** from the host-owned `FileAnalysisSnapshot` (the moved `css_definition_*`/`dom_query_*` logic), producing a SAME-FILE `DefinitionTarget::RealSource { uri: active_canonical, span: <SourceByteRange of the matched selector / template usage>, provenance: LiveSameFile }`. No tsgo, no typed-IR, no host-source snapshot (same file, position-preserving). The engine, not the LSP driver, owns this resolution.
- `terminalize(target)` — follow barrels to the terminal: `export { default as Foo } from './Foo.vue'` → that component's `SfcComponentAnchor` (fold in `resolve_barrel_locations` / `resolve_barrel_type_provider_location` in `server/component_resolve.rs`, ~`:668-756`; re-verify offsets at impl time).
- `dedup(targets)` — by **canonical identity** `(uri, target-kind, normalized source span, symbol identity)`, NOT suffix preference. Canonical source beats generated TSX for the same symbol. `.d.ts` kept only as the real terminal or when no real-source target exists. `.vue` never dropped merely because a non-`.vue` exists.

**5D. Normalize `TsDefinitionSpan` → `DefinitionTarget` (location-only; OQ-1; P1-1 coordinate-space match).** Match on the provenance-bearing `TsDefinitionSpan` enum (no `coordinate_space` field — the variant IS the coordinate space):
- `TsDefinitionSpan::GeneratedTsx { owner_canonical, generated_canonical, span: GeneratedByteRange, snapshot_id }` (a generated `.vue` surface) → **(P1-A)** the adapter already derived `owner_canonical` (the `.vue` source) — every generated `.vue.tsx`/`.vue.jsx` input is normalized through `owner_canonical_for_generated` (or dropped when it is not a host source) **before** any `target_ide_context` call, so the key passed in is always the owner `.vue` source canonical; fetch the target's IDE context via `target_ide_context(&owner_canonical)` (the host keys IDE context on the `.vue` source canonical — `target_ide_context` takes the owner `.vue` source canonical and returns `TargetIdeContext { source_canonical, generated_canonical, snapshot_id, .. }`, see Phase 4C); **(P1-I)** require `target_ide_context.snapshot_id == snapshot_id` else **DROP**; map the **generated** `span` (a `GeneratedByteRange`) → the **owner** `.vue` `SourceByteRange` via the target's host-sourced `PositionMapper`; produce `RealSource { uri: owner_canonical, provenance: GeneratedMapping(snapshot_id) }` (named binding) or `SfcComponent { uri: owner_canonical, provenance: GeneratedMapping(snapshot_id) }` (default/component) per whether the symbol is the component default. **Every `DefinitionTarget` constructed here carries `owner_canonical` — never the `.vue.tsx` generated key** — so all downstream normalize/render/dedup operate on the owner `.vue` identity (`target_ide_context` is keyed on `.vue` source canonicals, and dedup-by-canonical-identity (5C) must compare owner `.vue` ids, not generated `.vue.tsx` ids).
- `TsDefinitionSpan::HostSource { canonical_id, span: SourceByteRange, source_hash }` and `.ts/.js` → `RealSource { span, provenance: HostSource(source_hash) }` rendered via `target_source_context(target).line_index` (tsgo returns real byte offsets; render directly, **never** a Vue mapper).
- `TsDefinitionSpan::HostSource { .. }` and target library `.d.ts`/`.vue.d.ts` → `ExternalDeclaration { span, source_hash }` at the real declaration span (via `target_source_context`); **no** `.vue` fabrication.

**5E. LSP driver + render — `crates/verter_lsp/src/features/definition_engine.rs` (new, render-only) + `nav_features.rs`.**
- **(P1-3 + P1-B)** `verter_lsp` converts `tower_lsp::Position` → the typed `LspPosition` and assembles a `DefinitionRequest` carrying ONLY those typed coordinates (active `canonical_id` + `LspPosition` + the virtual-vs-source entry flag) — it does NOT classify, does NOT map Vue→TSX, and **does NOT call any nav leaf resolver** (no `css_definition_*`, no `dom_query_*`, no `try_*_definition`): every one of those moved into the engine. It supplies the `TsgoNavigationBackend`, calls `host`'s `DefinitionEngine::resolve(request, backend).await` (the engine classifies + maps + resolves CSS/DOM/same-file/cross-file internally), and **renders** each returned `DefinitionTarget` → `tower_lsp::Location`:
  - `RealSource{uri, span, provenance}` → validate the provenance token at render time (**P1-I**: mismatch → drop this target, do NOT recompute-and-remap): `GeneratedMapping(id)` ⇒ re-confirm `target_ide_context(uri).snapshot_id == id`; `HostSource(h)` ⇒ re-confirm `target_source_context(uri).source_hash == h`; `LiveSameFile` ⇒ no remote check. Then convert SFC-absolute `span` → LSP `Range` via the target's `LineIndex` from `host_source_context`/`host_target_context`. **Never** a Vue mapper for `.ts/.js`.
  - `SfcComponent{uri, anchor, provenance}` → validate provenance as above (a `SfcComponent` anchor is read from the target `.vue`'s own analysis, so it is `LiveSameFile`/`HostSource` unless it was reached via a generated mapping); render `anchor.preferred_span` via the target `.vue` `LineIndex` (SFC-absolute, position-preserving — no source-map needed).
  - `ExternalDeclaration{uri, span, source_hash}` → confirm `host_source_context(uri).source_hash == source_hash` (else drop); render the real `.d.ts` declaration span via that file's `LineIndex`. No fabricated `.vue`.
- **Rewire `handle_goto_definition` (`nav_features.rs:779-994`)** to call the engine: convert the position to typed coordinates, `DefinitionEngine::resolve(...).await`, render, return `Vec<Location>`. The **virtual `.vue.tsx` editing** branch (`:798-833`) also routes through the engine (architecture: "virtual `.vue.tsx` editing uses the same DefinitionEngine"): the LSP driver sets the `DefinitionRequest`'s entry flag to "virtual TSX" and passes the typed offset; **the engine's `classify`** (not the driver) recognises the virtual-TSX entry and emits a `TemplateSemantic` query, resolves via the injected nav backend, and renders through the same path.

### Legacy Deletions (in this phase — definition path only; the shared `merge_*` functions are deleted in Phase 6/7 where their LAST caller disappears)
- The cross-file **early return** in `handle_goto_definition` skipping tsgo when verter returns a cross-file scalar (`nav_features.rs:927-933`).
- The **call** to `merge_definitions_with_barrel_resolver` from `handle_goto_definition` (the function is deleted in Phase 6/7; its **use in definition** ends here).
- `try_precise_cross_file` + `resolved_import_definition` `Range::default()` fallback for `.vue` default (`definition.rs` `fn` at `:418`, `Range::default()` at `:422`) — superseded by engine `SfcComponent` rendering (`resolved_import_definition` deleted in Phase 6/7 when all callers are gone).
- The contract-prop **final fallback** pushing `Range::default()` in `try_component_contract_definition` (`server/component_resolve.rs`, inside the `fn` at `:375`; the `Range::default()` push ~`:493-498`, re-verify at impl time) — engine renders the child's `SfcComponentAnchor` instead of `(0,0)`.
- The virtual-file `Range::default()` branches in `handle_goto_definition` (`:825-829`) — replaced by engine routing.

### Tests (write FIRST; FAIL pre-change, PASS post-change)
**Rust engine tests = AUTHORITATIVE gate (OQ-6).** New `crates/verter_session/tests/definition_engine_tests.rs` (host-level, LSP-free — asserts the rendered `DefinitionTarget` span via a host `LineIndex`) and `crates/verter_lsp/tests/definition_render_tests.rs` (the LSP render → `Location` line/column). Each constructs a host + minimal committed fixtures:
- `vue_default_import_lands_on_component_anchor_not_first_binding` — **(anchor-not-first-binding)** parent imports `Child.vue` (leading helper then `defineOptions({name:'Child'})`); CTRL+CLICK the import → range == `Child` name span.
- `script_setup_target_anchor`, `explicit_export_default_target_anchor`, `template_only_target_anchor`, `define_options_target_anchor` — each lands on the expected priority anchor.
- `barrel_default_terminal_lands_on_terminal_vue_anchor` — `export { default as Foo } from './Foo.vue'` → Foo's anchor.
- `named_vue_export_lands_on_named_binding` — `export const x` in a `.vue` → x's real source span.
- `ts_export_lands_on_real_target_line_via_target_line_index` — import from `util.ts` → real line in `util.ts` (not 0:0, not via a Vue mapper).
- `library_dts_stays_dts` — a `@vueuse/core`-style symbol → its `.d.mts` declaration span (committed fixture `.d.ts`, NOT a third-party checkout — hermetic per Testing-Hermeticity); result is NOT rewritten to `.vue`.
- `missing_target_compile_yields_no_definition_not_zero_zero` — target cannot be compiled → no location (NOT `(0,0)`).
- `stale_mapper_snapshot_dropped_not_recomputed` — **(P1-I)** force a `CompileSnapshotId` mismatch → the target is DROPPED (assert no location rendered against the stale mapper; assert NO in-place recompute path runs).
- `dedup_keeps_canonical_vue_over_generated_tsx` — same symbol as both `.vue` and `.vue.tsx` → one `.vue` location; `.vue` NOT dropped because a non-`.vue` exists.
- `generated_tsx_hit_normalizes_to_owner_vue_canonical` — **(P1-A, discriminating)** drive a `TsDefinitionSpan::GeneratedTsx { generated_canonical = "…/Child.vue.tsx", owner_canonical = "…/Child.vue", .. }` through `normalize_ts_span`; assert the produced `DefinitionTarget`'s `uri` is the OWNER `…/Child.vue` (never the `.vue.tsx` generated key), the span is in `.vue` source coordinates, and dedup keys on the owner id. FAILS if normalization keys on or renders the `.vue.tsx` canonical, or if the owner derivation is skipped.
- `same_file_binding_renders_sfc_absolute_span` — template `{{ count }}` → `count` decl line/col in the same file.
- `css_class_in_template_navigates_to_style_selector` — **(P1-B, engine-routed)** cursor on `class="btn"` in the template → the `.btn` selector span in `<style>` (same-file `RealSource`/`LiveSameFile`); resolved through the engine's `CssSelectorFromTemplate` query, NOT a LSP-driver leaf. Discriminating: fails if CSS resolution does not run inside the engine.
- `css_selector_in_style_navigates_to_template_usage` — **(P1-B)** cursor on `.btn` in `<style>` → the `class="btn"` template usage span; engine `CssSelectorFromStyle`.
- `dom_query_selector_navigates_to_element` — **(P1-B)** cursor on the selector literal inside a `querySelector('.btn')` call → the matching element/selector span; engine `DomQuerySelector` (+ its CSS fallback). Discriminating: fails if DOM-query resolution does not run inside the engine.
- `definition_does_not_dispatch_typed_ir` — **(OQ-1 guard 4, runtime; NIT-2)** install an `AuditObserver` (or a capture token); run a full go-to-def over every fixture above; assert **ZERO** `SemanticQueryKey`/`ProjectSemanticDispatch` dispatches and **no** type expansion occurred — counted via the per-request dispatch counter `record_dispatch`/`record_dispatch_warm` (`semantic_query_memo/mod.rs:2359`/`:2416`/`:2353`, reached through `with_active_capture`) or the `AuditObserver` counter, NOT via `current_observer()` at `:3818` (a doc-comment mention). Discriminating: routing definition through typed-IR increments the counter and trips it.

**E2E (OQ-6, non-skippable)** in `packages/vue-vscode/e2e/suite/definition.test.ts` over the committed fixture workspace `packages/vue-vscode/e2e/fixtures/goto-definition/` — **line+column** assertions:
- Component-anchor: CTRL+CLICK a child component tag/import lands on the child's `defineOptions`/`<script setup>`/template-root line+column.
- The Phase-2 D-series (v-html `msg`, `:[key]` `val`, native `v-model` `count`) now assert exact source line+column end-to-end.
- **Replace `this.skip()` with marker assertions that THROW on missing marker** (OQ-6/P2-D) so no discriminating assertion silently skips.

### Architecture guards
- `definition_routes_through_engine` (source scan): `handle_goto_definition` body contains no direct `merge_definitions_with_barrel_resolver` call and no `Range::default()` construction.
- **(P1-B)** `lsp_definition_driver_calls_no_nav_leaf_resolver` (static, in `architecture_guards.rs`): scan the `verter_lsp` definition driver (`features/definition.rs`, `features/definition_engine.rs`, `server/nav_features.rs` definition/type-def/refs/rename handlers) and assert it calls **no** nav leaf resolver — no `css_definition_*`, no `dom_query_*` (`dom_query_definition`/`dom_query_css_fallback`), no `find_css_*`, no `try_*_definition`, no `definition_at_position` — every one now lives in `verter_session::navigation::definition` and the driver only renders engine `DefinitionTarget`s. Discriminating: re-introducing any `css_definition_*`/`dom_query_*`/nav-resolver call in the LSP driver fails the scan. (Complements `direct_tsgo_nav_only_in_adapter` — together they prove the driver is render-only: no tsgo nav, no CSS/DOM/same-file leaf resolution.)
- **(P1-3)** `classification_and_mapping_live_in_engine` (static): the `classify` → `DefinitionQuery` logic and the Vue→TSX `LspPosition`→`GeneratedByteOffset` mapping symbols exist in `verter_session::navigation::definition` (the engine) and are ABSENT from the `verter_lsp` definition driver — assert `verter_lsp::features::definition_engine`/`nav_features::handle_goto_definition` neither builds a `DefinitionQuery` nor calls a `PositionMapper` to map the active position into TSX (the driver only constructs typed `LspPosition` request coordinates and renders). Discriminating: moving classification or Vue→TSX mapping back into the LSP driver fails the scan.
- **OQ-1 guard 1** `ts_navigation_backend_exposes_no_type_payloads` (unit/static): assert **every** `TsNavigationBackend` return type — `TsDefinitionSpan`, `TsRenameEdit`, `TsCodeActionEdit` (P1-2) — and the trait's four methods have no field/return of `TypeExpr`/`SemanticNodeId`/checker types/member lists/quickinfo (introspect the types; a re-introduced type payload on ANY of the four methods fails).
- **OQ-1 guard 2** `nav_modules_do_not_import_type_resolution` (static, in `architecture_guards.rs`): scan the **declaration-site** nav modules — `verter_session::navigation::definition` (+ the `references`/`rename` nav modules) and `verter_lsp::features::definition*`/`definition_engine` — for imports/calls of `SemanticQueryKey`/`ProjectSemanticDispatch`/`SemanticGraphStore`/type materializers/tsgo type APIs and assert **absent** (these surfaces are strictly **zero-dispatch**). **(P2-A) Named allowlist — EXACTLY one module:** `verter_session::navigation::type_definition` is **exempt** from this ban, because type-definition legitimately routes its type computation through `ProjectSemanticDispatch` (the single typed-IR engine) per OQ-7 — that is the ONE allowed typed-IR call site in `verter_session::navigation::**`. The guard therefore scopes its scan to the declaration-site nav modules and explicitly names `navigation::type_definition` as the sole allowlisted nav module, so (a) a re-introduced dispatch in definition/references/rename FAILS, and (b) the type-definition module's intentional `ProjectSemanticDispatch` use does not trip the guard. The new `navigation::type_definition` module owns ONLY the type-valued lookup (resolving the symbol's *type* declaration location to a `CompilerLocation`); it does NOT classify or render plain declaration locations (those stay in `navigation::definition`). The shared render/normalize core in `navigation::definition` (6/7-A) remains zero-dispatch — type-definition calls into it AFTER its `ProjectSemanticDispatch` step returns a `CompilerLocation`.
- **OQ-1 guard 3** `direct_tsgo_nav_only_in_adapter` (static): direct tsgo nav — `type_provider.get_definition`/`get_references`/`get_rename_locations`/`get_code_actions` (the four location-only nav calls) — appears ONLY in `verter_lsp::tsgo::TsgoNavigationBackend`; all LSP handlers (definition, references, rename, code-actions) route through `DefinitionEngine` + its `TsNavigationBackend` siblings. (`get_type_definition` is exempt from this nav guard: type-definition routes through `ProjectSemanticDispatch`, not this backend — see 6/7-B.)
- **OQ-5 guard** `no_lsp_types_in_session_nav` (static): `verter_session::navigation::**` does not import `tower_lsp`/`lsp_types` (assert no such use; a leaked LSP type fails).
- `ban_range_default_in_nav_construction` — see Phase 6/7 (registered here, fully enforced there with the explicit allowlist).
- Register all OQ-1/OQ-5 guards + the new CRITICAL section in `tests/g_misc0/critical_rules_have_guards.rs` (so the R6 meta-guard passes).

### Verification
```
cargo nextest run --workspace 2>&1 | tee /tmp/p5.txt
cargo test -p verter_session --tests 2>&1 | tee -a /tmp/p5.txt
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
pnpm build:native && pnpm build:lsp
```
Expected: green; definition navigation lands on exact targets across all per-target kinds; the four desync constructs navigate end-to-end; zero typed-IR dispatch during go-to-def.

---

## PHASE 6/7 (MERGED — P0-3) — Route ALL nav surfaces through the engine AND delete legacy arbitration in one landable change

### Context (P0-3 + OQ-7 BINDING)
**P0-3:** Phase 6 (delete arbitration) and Phase 7 (route type-def/refs/rename/code-actions) **cannot** land separately: the shared `merge_*` functions' last callers live in the type-def/refs/rename handlers, so deleting them before those handlers are rewired leaves a caller (a retired-symbol guard could not pass). They are therefore **one merged phase**: every nav surface is routed through the engine's shared `render`/`normalize` core, **then** the now-caller-free legacy functions are deleted — all in the same commit. No phase deletes a symbol whose callers still exist.

**OQ-7 (semantic split):** definition/references/rename may use the **location-only** `TsNavigationBackend`; **type-definition MUST route the type computation through `ProjectSemanticDispatch`** (the single typed-IR engine) — only the final **rendering** is shared. Every compiler-derived location is a `CompilerLocation` whose coordinate-space + provenance is **intrinsic to its variant** (`Generated{snapshot_id}` | `RealSource{source_hash}`, P1-1) — the type makes a provenance-less location unrepresentable, and the renderer DROPS any location whose intrinsic token no longer matches the live host.

### Changes

**6/7-A. Generalize the engine's shared render/normalize core (in `verter_session::navigation::definition`).**
- Extract `render_target(target) -> Option<RenderedSpan>` and `normalize_ts_span(TsDefinitionSpan, …) -> Option<DefinitionTarget>` as the shared core (LSP-free; the LSP driver converts `RenderedSpan` → `tower_lsp::Location`).
- New **`CompilerLocation`** (OQ-7 + P1-1) — the common provenance-carrying value every backend (nav or semantic) normalizes into a `DefinitionTarget`. **(P1-1)** It is modelled by coordinate-space + provenance so the "span with two bare `Option`s" hazard is unrepresentable — a generated span ALWAYS carries a `CompileSnapshotId`, a real-source span ALWAYS carries a `Hash16`:
  ```rust
  pub enum CompilerLocation {
      // `Generated`'s canonical is the OWNER `.vue` source canonical (`owner_canonical`),
      // never the `.vue.tsx` generated key — generated inputs are normalized through
      // `owner_canonical_for_generated` (or dropped) before a `CompilerLocation::Generated`
      // is constructed, so `target_ide_context(&owner_canonical)` keys correctly.
      Generated { owner_canonical: CanonicalId, span: GeneratedByteRange, snapshot_id: CompileSnapshotId },
      RealSource { canonical_id: CanonicalId, span: SourceByteRange, source_hash: Hash16 },
  }
  ```
  `normalize_ts_span`/normalization consumes a `CompilerLocation` whose provenance is intrinsic to its variant — there is no "neither" state to reject (the type forbids it). `Generated` validates against `target_ide_context(&owner_canonical).snapshot_id` (its canonical is the owner `.vue`, normalized via `owner_canonical_for_generated` before construction) and maps `span` → `.vue` `SourceByteRange`; `RealSource` validates against `target_source_context(canonical_id).source_hash` and renders directly.

**6/7-B. Route type-definition through `ProjectSemanticDispatch` (OQ-7) — new `verter_session::navigation::type_definition` module + `handle_goto_type_definition` (`nav_features.rs:997-1109`).**
- Type-definition needs a **type-valued** answer (the declaration of a symbol's *type*), so its type computation lives in a **dedicated `verter_session::navigation::type_definition` module** (LSP-free, like the definition core) that routes through the shared `SemanticQueryKey → ProjectSemanticDispatch → SemanticGraphStore` engine (the single typed-IR resolver), obtaining the type's declaration location as a `CompilerLocation`, then **renders via the shared core** (6/7-A, in `navigation::definition`). This is the ONE nav surface — and `navigation::type_definition` the ONE nav module (P2-A guard allowlist) — that legitimately touches typed-IR; it is computing a type, not a plain declaration location, consistent with the single-engine rule. `handle_goto_type_definition` becomes a thin driver: convert position → typed coords, call the `navigation::type_definition` entry, render each `CompilerLocation` via the shared core.
- Delete its virtual-file `Range::default()` branch (`:1035-1044`) and its `merge_definitions_with_barrel_resolver` call (`:1083-1095`).

**6/7-C. Route references/rename/code-actions through the location-only backend siblings + shared render — `merge.rs` + handlers.** Each surface calls its `TsNavigationBackend` sibling method (5A, P1-2), normalizes each opaque provenance-bearing result via `normalize_ts_span` (the same provenance-validate-or-DROP path as definition), and renders via the shared core — NOT the deleted `merge_*` arbitration.
- `handle_references` → call `TsNavigationBackend::references_at_position(...)`, map each returned `TsDefinitionSpan` through `normalize_ts_span` + `render_target`. **Delete** `merge_references`'s `.vue.d.ts`/`.vue.ts` `Range::default()` and external `.ts/.js` `Range::default()` branches (the function is retired in 6/7-D; the existing `merge_references_vue_dts_maps_to_vue` test at `merge.rs:2509` characterizes the `.vue.d.ts` behavior and is rewritten to assert the real-range render). Keep the `Vec<Location>` shape; dedup by canonical identity.
- `handle_rename` → call `TsNavigationBackend::rename_at_position(...)`; each `TsRenameEdit` normalizes its `location` (real span, provenance-validated) and renders to a `TextEdit`; a rename edit targets the **real** binding/anchor span, never 0:0. Delete `merge_rename_locations`'s `.vue.d.ts`/external `.ts/.js` `Range::default()` fallbacks (retired in 6/7-D).
- `handle_*` code-action → call `TsNavigationBackend::code_actions_at_position(...)`; each `TsCodeActionEdit`'s edits normalize their navigable target/range through the shared core; **(P2-E)** delete the external `.ts/.js` + `.vue.d.ts`/`.vue.ts` code-action `Range::default()` fallbacks in `merge_code_actions` (`merge.rs:1136` for `.vue.d.ts`/`.vue.ts`, `:1142` for external `.ts/.js` — **both deleted**, see P1-4). **(Round-3 P2-2 correction)** `extract_component.rs:118` is **NOT** in this migration set — it is the `range` of a `TextEdit` that writes the FULL CONTENT of a brand-new `.vue` file (`extract_component_action` → `CreateFile { new_file_uri }` → `TextEdit { new_text: new_component_source }`; the comment reads "Write content to the new file"). For a not-yet-existent empty file, `Range::default()` (0,0) is the CORRECT, intentional insertion point — it is a REFACTOR_EXTRACT edit, NOT a go-to-definition navigation result, and NOT in the merge.rs wrong-0:0 defect class. It is therefore a **documented non-nav allowlist exception** (justification: new-file content insertion, (0,0) is correct), NOT migrated — see 6/7 Legacy Deletions and the guard below. **(Round-3 P2-B)** `action_utils.rs:652` is **NOT** in this set either — it lives inside `#[cfg(test)] mod tests` (test module starts at `:437`), so it is TEST and there is nothing to migrate.

**6/7-D. Delete now-caller-free legacy arbitration (all callers removed above).**
- `resolve_vue_tsx_range` (`merge.rs` `fn` at `:647`) — the current-file mapper fallback + its trailing `.unwrap_or_default()` (at `merge.rs:681`). **Delete** (last callers: `merge_definitions_with_barrel_resolver`, `merge_references` — both rewired above). (Note: the literal `Range::default()` at `merge.rs:786` is NOT in this function — it is the else-arm default inside `merge_definitions_with_barrel_resolver` (`:725-815`); `resolve_vue_tsx_range`'s own default is the `.unwrap_or_default()` at `:681`.)
- `merge_definitions_with_barrel_resolver` (`merge.rs` `fn` at `:725`) — entire verter/tsgo arbitration: the non-`.vue.tsx`/`.vue.jsx` else-arm `Range::default()` (at `:786`), the "prefer non-`.vue`" suffix dedup, the dedup-by-URI (call-site offsets re-verified at impl time). **Delete** (definition stopped calling it in Phase 5; type-def stops in 6/7-B).
- `merge_references` (`merge.rs` `fn` at `:874`, with `Range::default()` at `:913`/`:921`), `merge_rename_locations` (`merge.rs` `fn` at `:943`, with `Range::default()` at `:989`/`:996`), and `merge_code_actions` (`merge.rs` `fn` at `:1102`, with `Range::default()` at `:1136`/`:1142`). **Delete** all three (last callers: the references/rename/code-action handlers, rewired in 6/7-C to call `references_at_position`/`rename_at_position`/`code_actions_at_position` + `normalize_ts_span` + the shared render core). Their `.vue.d.ts`/`.vue.ts` + external `.ts/.js` `Range::default()` branches go with them — this is what makes the merge.rs PRODUCTION `Range::default()` set EMPTY after §6/7 (P1-4).
- `resolved_import_definition` (`features/definition.rs` `fn` at `:418`) — returns `Range::default()`. **Delete** (callers removed in Phase 5).
- `definition_at_position` (`features/definition.rs` `fn` at `:41`), `try_component_contract_definition` (`server/component_resolve.rs` `fn` at `:375`), `try_barrel_export_definition` (`server/component_resolve.rs` `fn` at `:584`) — logic **moved into** `DefinitionEngine::classify` (Phase 5). Delete the standalone orchestrators.
- **(P1-B) CSS/DOM leaf resolvers — ROUTE (move into the engine), do NOT delete the feature.** The logic in `features/definition.rs` — `css_definition_from_template` (`fn` at `:479`), `find_css_target_in_template` (`:495`), `find_css_selector_definition` (`:559`), `css_definition_from_style` (`:588`), `find_css_target_in_style` (`:629`), `dom_query_definition` (`:735`), `dom_query_css_fallback` (`:774`), and their call sites in `definition_at_position` (`:228`/`:391`/`:405`) — **moves into** `verter_session::navigation::definition` (resolving the new `CssSelectorFromTemplate`/`CssSelectorFromStyle`/`DomQuerySelector` queries, Phase 5/5C). The CSS↔template / DOM-query NAVIGATION is preserved (real features); only its HOME changes (LSP driver → engine). After the move, the standalone `css_definition_*`/`dom_query_*` functions are **deleted from `features/definition.rs`** (the LSP driver no longer calls them — it only renders engine `DefinitionTarget`s); they are added to the `RETIRED_DEFINITION_SYMBOLS` registry so a re-introduced LSP-driver copy fails.
- `external_ide_context` (`provider_state.rs:26-40`) + `ide_context_by_path` (`sync_orchestration.rs:1380-1389`) — the open-doc-dependent cross-file readers; their last callers (the `ExternalIdeResolver` closures in the merge functions) are now gone. **Delete** (replaced by `host_target_context`/`host_source_context`).

### Legacy Deletions (consolidated — every item, with the file:line where its last caller disappears)
- `handle_goto_type_definition` virtual-file `Range::default()` branch (`nav_features.rs:1035-1044`).
- `merge_references` (deleted whole; `fn` at `merge.rs:874`, `Range::default()` at `:913`/`:921`).
- `merge_rename_locations` (deleted whole; `fn` at `merge.rs:943`, `Range::default()` at `:989`/`:996`).
- `merge_code_actions` (deleted whole; `fn` at `merge.rs:1102`, `Range::default()` at `:1136`/`:1142`). **(Round-3 P2-2 correction)** `extract_component.rs:118` is **NOT** deleted/migrated here — it is a new-file content-insertion `TextEdit` range (write full content into a freshly-`CreateFile`d `.vue`), where `Range::default()` (0,0) is correct; it is a REFACTOR_EXTRACT edit, not a nav result, and is a **documented non-nav allowlist exception** (alongside `workspace_symbol.rs`). **(Round-3 P2-B)** `action_utils.rs:652` is **excluded** here — it is inside `#[cfg(test)] mod tests` (test module at `:437`), i.e. TEST, with nothing to migrate.
- `resolve_vue_tsx_range` (`merge.rs` `fn` at `:647`, trailing `.unwrap_or_default()` at `:681`), `merge_definitions_with_barrel_resolver` (`merge.rs` `fn` at `:725`, its else-arm `Range::default()` at `:786`), `resolved_import_definition` (`features/definition.rs` `fn` at `:418`), `definition_at_position` (`features/definition.rs` `fn` at `:41`), `try_component_contract_definition` (`server/component_resolve.rs` `fn` at `:375`), `try_barrel_export_definition` (`server/component_resolve.rs` `fn` at `:584`).
- **(P1-B)** the CSS/DOM leaf resolvers in `features/definition.rs` (`css_definition_from_template:479`, `find_css_target_in_template:495`, `find_css_selector_definition:559`, `css_definition_from_style:588`, `find_css_target_in_style:629`, `dom_query_definition:735`, `dom_query_css_fallback:774`) — their logic is **moved into** `verter_session::navigation::definition` (Phase 5); the standalone LSP-driver copies are deleted once the engine resolves the `CssSelectorFromTemplate`/`CssSelectorFromStyle`/`DomQuerySelector` queries (last callers — the `:228`/`:391`/`:405` sites in `definition_at_position` — go with that function).
- `external_ide_context` (`provider_state.rs:26-40`), `ide_context_by_path` (`sync_orchestration.rs:1380-1389`).
- **(P1-4)** After all the above, the merge.rs PRODUCTION `Range::default()` carry-forward set is **EMPTY**. The remaining merge.rs `Range::default()` occurrences (`:1647`/`:1705`/`:1785`/`:1820`/`:2023`/`:2104`/`:2119`/`:3060`) are ALL inside `#[cfg(test)] mod tests` (starts `:1318`) and are out of scope by the standard test exemption.

### Tests (write FIRST; FAIL pre-change, PASS post-change)
- `type_definition_external_target_renders_real_range` — type-def on a prop typed from a `.ts` interface → real line/col in the `.ts` (not 0:0).
- `type_definition_routes_through_project_semantic_dispatch` — **(OQ-7)** install the audit observer; assert type-def DOES dispatch `SemanticQueryKey`/`ProjectSemanticDispatch` (the type computation), and that definition/references/rename do NOT (the OQ-1 split is real, not accidental).
- `references_cross_file_render_real_ranges` — references of a symbol used in a closed `.vue` consumer → real ranges (not 0:0).
- `references_dedup_by_canonical_identity` — a symbol surfaced as both `.vue` and `.vue.tsx` → one `.vue` ref.
- `rename_targets_real_span_not_zero_zero` — rename edit lands on the real binding/anchor span.
- `type_definition_dedup_keeps_canonical` — mirrors the definition dedup rule.
- `compiler_location_carries_intrinsic_provenance` — **(OQ-7 + P1-1, discriminating)** assert the `CompilerLocation` type makes the "no provenance" state unrepresentable: `Generated` carries a `CompileSnapshotId` and a `GeneratedByteRange`, `RealSource` carries a `Hash16` and a `SourceByteRange` (introspect the variants — a re-introduced `Option<CompileSnapshotId>`/bare-`coordinate_space` shape fails the type-level assertion). PLUS a runtime arm: a `Generated` location whose `snapshot_id` does NOT match `target_ide_context(uri).snapshot_id` is DROPPED by `normalize_ts_span` (cannot produce a `DefinitionTarget`); a `RealSource` location whose `source_hash` mismatches is likewise DROPPED. Discriminating: dropping the provenance check makes a stale location render.
- `cross_file_target_no_longer_zero_zero` — the exact scenarios that previously hit `Range::default()` (closed target `.vue`, library `.d.ts`, external `.ts`) now render a real range or no-result.
- E2E: extend `references.test.ts`, `rename.test.ts`, add a type-definition assertion (and `code-actions.test.ts` where applicable) checking **line+column**, not just file; non-skippable marker assertions over the committed fixtures.

### Architecture guards
- `all_definition_surfaces_route_through_engine` (architecture-named): assert (source scan) **definition, type-definition, references, rename, code-actions** all call the shared engine/render path and none constructs `Range::default()` or calls a deleted `merge_*` arbitration.
- **(P1-H + P1-4 + round-3 P2-B/P2-2, RESOLVED EXPLICITLY)** `ban_range_default_in_nav_construction` (static): ban `Range::default()` in **navigation-result construction** scoped to the engine-routed handler/merge functions: `nav_features.rs` (`handle_goto_definition`, `handle_goto_type_definition`, `handle_references`, `handle_rename`, code-action handler), `merge.rs` definition/reference/rename/**code-action** paths (incl. the deleted `merge_code_actions` `:1136`/`:1142`), `features/definition*.rs`, `features/definition_engine.rs`, and `component_resolve.rs`. **`extract_component.rs` is EXCLUDED from this guard's scope (its `:118` `Range::default()` is a new-file content-insertion `TextEdit` range, not a nav result).** **Allowlist (with justification, NOT migrated) — TWO documented non-nav exceptions (P2-B + P2-2):** (1) `features/workspace_symbol.rs` (PRODUCTION sites `:47`/`:67`/`:92`/`:114`) — symbol-index ranges, **not** go-to-def navigation results, out of this plan's scope (tracked as CF-1); and (2) `features/extract_component.rs` (PRODUCTION site `:118`) — the `range` of a `TextEdit` that writes the FULL CONTENT of a brand-new `.vue` file (`CreateFile { new_file_uri }` then `TextEdit { range: Range::default(), new_text: new_component_source }`), where `(0,0)` is the CORRECT insertion point for a not-yet-existent empty file (a REFACTOR_EXTRACT edit, not a navigation result). The guard scans only **production** source (it skips `#[cfg(test)]` modules), so the merge.rs test-module sites (`:1647`/`:1705`/`:1785`/`:1820`/`:2023`/`:3060`), the `call_hierarchy.rs` `:290`/`:291` (inside `mod tests` at `:219`), and `action_utils.rs:652` (inside `mod tests` at `:437`) need **no** allowlist — the standard test exemption covers them. The guard enumerates the **two-file** allowlist explicitly so a NEW `Range::default()` in any nav path fails, while the two documented non-nav files (`workspace_symbol.rs` + `extract_component.rs`) are exempt — either via the allowlist entry for `extract_component.rs` or by excluding `extract_component.rs` from the guard's file scope (both achieve the same exemption with the new-file-insert justification). **(P1-4)** The code-action `Range::default()` defaults in `merge_code_actions` (`:1136`/`:1142`) ARE migrated/deleted in 6/7-C and are **NOT** allowlisted; `extract_component.rs:118` is **NOT** migrated — it is a documented non-nav exception.
- `RETIRED_DEFINITION_SYMBOLS` registry (new `crates/verter_lsp/tests/no_legacy_definition_arbitration.rs`, mirroring the `RETIRED_SYMBOLS` pattern in `architecture_guards.rs`): every deleted function name (`resolve_vue_tsx_range`, `merge_definitions_with_barrel_resolver`, `merge_references`, `merge_rename_locations`, `merge_code_actions`, `resolved_import_definition`, `external_ide_context`, `ide_context_by_path`, `definition_at_position`, `try_component_contract_definition`, `try_barrel_export_definition`, **and the P1-B CSS/DOM leaf resolvers now moved into the engine: `css_definition_from_template`, `find_css_target_in_template`, `find_css_selector_definition`, `css_definition_from_style`, `find_css_target_in_style`, `dom_query_definition`, `dom_query_css_fallback`**) — the test fails if any reappears in `crates/verter_lsp/src/**`.
- `ban_suffix_based_dedup` (source scan): no `.retain(|l| !l.uri.as_str().ends_with(".vue"))`-style suffix-preference dedup in nav code.
- **(OQ-6/P2-D)** `ban_this_skip_in_definition_suite` (source scan over `packages/vue-vscode/e2e/suite/definition.test.ts` + the new nav suites): assert no `this.skip()` remains in the go-to-def/type-def/references/rename suites — every discriminating assertion runs against the committed fixtures or THROWS on a missing marker. Discriminating: re-introducing a `this.skip()` (which would silently pass) fails the guard.
- `ban_open_doc_mapper_for_cross_file_targets` (Phase 4) — now **fully enforced** (no remaining violations).
- `direct_tsgo_nav_only_in_adapter` / `nav_modules_do_not_import_type_resolution` (Phase 5) — re-asserted across all nav surfaces; **(P2-A)** `verter_session::navigation::type_definition` is the ONE allowlisted nav module whose `ProjectSemanticDispatch` use is the ONE allowed typed-IR call site (its own narrow, module-named allow, distinct from the strictly zero-dispatch declaration-site nav modules definition/references/rename).

### Verification
```
cargo nextest run --workspace 2>&1 | tee /tmp/p67.txt
cargo test -p verter_session --tests 2>&1 | tee -a /tmp/p67.txt
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile
pnpm test
pnpm build:native && pnpm build:lsp
pnpm --filter @verter/vue-vscode test:e2e
node scripts/gen-corpus-audit-tests.mjs   # if audit fixtures touched
```
Expected: green across Rust + TS + e2e; all five navigation surfaces share one rendering path; no `Range::default()` remains in nav-result construction outside the documented allowlist; legacy symbols gone and guarded; definition/references/rename do zero typed-IR dispatch; type-definition routes type computation through `ProjectSemanticDispatch`.

---

## DOCUMENTATION (lands with Phase 6/7)
- `/position-encoding` skill — the typed coordinate wrappers (`SourceByteOffset`/`GeneratedByteOffset`/`GeneratedByteLen`/`SourceByteRange`/`SourceUtf16Offset`/`GeneratedUtf16Offset`/`LspPosition`/`TsPosition`) and the strict `Option`-returning `PositionMapper` contract (within-run precision retained; cross-token extrapolation banned).
- `/compiler-codegen` skill — `EmitOp`/`emit.rs`/`JsxBindingValue` and the "IDE codegen never bakes prefix+identifier into a mapped overwrite" rule + exact per-op lowering; reference the v-for/static-key mirror patterns.
- `/host-session` skill — `CompileSnapshotId` (the compile warm-hit fact-validated identity `Hash128`, per §3.1.2), `target_ide_context`/`target_source_context`, host-sourced target mappers, snapshot-match-or-DROP rule.
- `/component-meta` skill — `SfcComponentAnchor` (priority 1→5) + `TemplateAnalysisState` as first-class `.vue` analysis output at `IndexedReady`; `find_export_span` heuristic removal.
- `/type-resolution` skill — the OQ-1 boundary: declaration-site navigation uses the location-only `TsNavigationBackend`; type-definition routes type computation through `ProjectSemanticDispatch` (the single typed-IR engine); definition/references/rename never dispatch typed-IR.
- `CLAUDE.md` — add a `(CRITICAL)` "Go-to-Definition Navigation Engine" section (single `DefinitionEngine`; strict mapper; anchors; host-sourced snapshot-validated mappers; location-only nav backend vs typed-IR for type-definition; no nav `Range::default()`), and register it in **`crates/verter_session/tests/g_misc0/critical_rules_have_guards.rs`** `CRITICAL_RULE_GUARDS` with the named guards above. **(NIT)** Also fix the stale `CLAUDE.md` pointer that referenced `tests/critical_rules_have_guards.rs` — the real path is `tests/g_misc0/critical_rules_have_guards.rs`.
- `AGENTS.md` — update skill routing for the new section/guard registry pointer.

---

## § FINDINGS RESOLUTION MAP (every P0/P1/P2/NIT + every OQ → resolution + phase)

| Finding | Resolution | Phase |
|---|---|---|
| **P0-1** one-engine (OQ-1 tie-break) | `DefinitionEngine` = nav orchestrator; location-only `TsNavigationBackend`→`TsDefinitionSpan`; 4 guards; type-def via `ProjectSemanticDispatch`; NO new `SemanticQueryKey` mode | 5, 6/7 |
| **P0-2** CompileSnapshotId derivation | **Reconciled to §3.1.2**: `CompileSnapshotId = Hash128(compile warm-hit identity)` = `semantic_hash` + style/content override hashes + profile + parser/compiler/version + canonicalized `ReadSetSignature` facts (the cross-file fact signature catches stale-stability under cross-file dep edits); the TSX-byte hash is debug/collision metadata only; `.ts/.js/.d.ts` validated via `TargetSourceContext.source_hash`, not a TSX id | 4 |
| **P0-3** Phase 6 not independently landable | **Phases 6+7 MERGED**: route all nav surfaces, THEN delete caller-free symbols, same commit; no symbol deleted while callers exist | 6/7 |
| **P1-A** EmitOp not typed | `emit.rs` with typed wrappers + `MoveOriginal` + `anchor`; deviation note (4 sites need no move) | 2 |
| **P1-B** prefix lowering contradictory | Exact per-op lowering; pick separate-prefix + `content_offset=0`; `vmodel_prefix_not_double_shifted` test | 2 |
| **P1-C** OverwriteSyntheticBoundary maps at start | Lowered as delete + unmapped `prepend_static` insert → synthetic text maps to None; `synthetic_boundary_start_maps_to_none` test | 2 |
| **P1-D** §1B replace-not-delete within-run | Delete only cross-token (`:56-109`); replace unconditional deltas with within-run guarded delta; within-run precision tests both directions | 1 |
| **P1-E** (+ **NIT-3**) Phase 1 caller list incomplete | Added `merge_semantic_tokens` (`:1183`/`:1198`), `merge_inlay_hints` (`:1283`), **17** `integration_tests.rs` sites (ipc.rs `__lsp_tests` copies dead/out-of-scope); reframed "already Option, change is typed inputs + delete Some-extrapolation" | 1 |
| **P1-F** defineOptions name-span field | New `AnalyzedMacro.define_options_name_span` producer field captured in shallow analysis | 3 |
| **P1-G** anchor site + OQ-3 | Anchor at `IndexedReady`; non-optional `TemplateAnalysisState`; template-root from SFC syntax; priority-4-discriminating guard (TemplateRootStart not FileStart) | 3 |
| **P1-H** (+ **P1-4** + **round-3 P2-B/P2-2**) ban_range_default unsatisfiable | Guard scoped to engine-routed handler/merge fns (production-only scan); allowlist = TWO documented non-nav files: `workspace_symbol.rs` (CF-1, symbol-index ranges) + `extract_component.rs:118` (new-file content-insertion `TextEdit` range, (0,0) correct — NOT a nav result); `call_hierarchy.rs` `:290`/`:291` + `action_utils.rs:652` are inside `mod tests` → TEST, exempt by standard test rule (NOT allowlisted, nothing to migrate); merge.rs `:1136`/`:1142` are code-action NAV defaults → deleted (not allowlisted); merge.rs `:1647`+ are test-module sites → exempt; `extract_component.rs:118` → documented non-nav allowlist exception (NOT migrated) | 6/7 |
| **P1-I** stale snapshot = DROP | render() DROPs on mismatch; no recompute-in-place-and-remap; `stale_mapper_snapshot_dropped_not_recomputed` test | 5, 6/7 |
| **P1-A** (round-3) generated-target owner canonical | `TsDefinitionSpan::GeneratedTsx` carries `{ owner_canonical, generated_canonical, span, snapshot_id }`; `TargetIdeContext` carries `{ source_canonical, generated_canonical, snapshot_id }`; new `VerterHost::owner_canonical_for_generated` (path-suffix strip + `get_source` guard) is the sole generated→owner derivation, called by the adapter; normalize/render/dedup key on the OWNER `.vue`; tests `target_ide_context_carries_both_canonicals`, `owner_canonical_for_generated_strips_suffix`, `generated_tsx_hit_normalizes_to_owner_vue_canonical` | 4, 5 |
| **P1-B** (round-3) CSS/DOM definition dual path | CSS-selector + DOM-query nav modeled as engine-owned `DefinitionQuery::{CssSelectorFromTemplate,CssSelectorFromStyle,DomQuerySelector}` in `verter_session::navigation::definition` (logic MOVED from `features/definition.rs:479-…`, operates on host-owned `FileAnalysisSnapshot` template/styles/dom_query_calls → same-file `LiveSameFile` targets); LSP driver classifies-via-engine + renders only (no `css_definition_*`/`dom_query_*` calls); new guard `lsp_definition_driver_calls_no_nav_leaf_resolver`; CSS/DOM leaf fns added to `RETIRED_DEFINITION_SYMBOLS`; tests `css_class_in_template_navigates_to_style_selector`/`css_selector_in_style_navigates_to_template_usage`/`dom_query_selector_navigates_to_element` (engine-routed). ROUTE, not delete | 5, 6/7 |
| **P1-J** one-engine guard missing | 4 guards (OQ-1) + critical-rule registry entry | 5, 6/7 |
| **P2-A** (round-3) one-engine guard vs type-def | Guard 2 (`nav_modules_do_not_import_type_resolution`) scoped to the zero-dispatch declaration-site nav modules (definition/references/rename); **named allowlist = EXACTLY one module `verter_session::navigation::type_definition`** (the ONE legitimate `ProjectSemanticDispatch` call site per OQ-7); type-def's type computation lives in that dedicated module + renders via the shared zero-dispatch core | 5, 6/7 |
| **P2-A** v-model reverse selection | Deterministic "first covering mapped run in generated order = read occurrence"; `vmodel_source_to_generated_selects_read_occurrence` test | 2 |
| **P2-B** CRLF/tabs/multiline tests | Added mapper tests (`test_crlf_mapping`/`test_tabs_mapping`/`test_multiline_mapped_expression`) + codegen `emit_codegen_crlf_and_tabs` | 1, 2 |
| **P2-C** retired-symbol guards + full callers | Full 11+3 caller enumeration; migrate all; `RETIRED_IDE_EMIT_SYMBOLS` name guard | 2 |
| **P2-D** e2e non-skippable | Committed fixtures `e2e/fixtures/goto-definition/`; `this.skip()`→throwing marker assertions; source guard banning `this.skip()` in definition suite | 5, 6/7 |
| **P2-E** (+ **P2-2** + **round-3 P2-B**) Phase 7 deletions vague | Named functions/files for external `.ts/.js` + code-action `Range::default()` (`merge_code_actions:1136`/`:1142`) — **MIGRATED/deleted, never allowlisted** (nav-result defaults); added to retired-symbol/source-scan guard. **(P2-2 correction)** `extract_component.rs:118` is a new-file content-insertion `TextEdit` range ((0,0) correct), NOT a nav result → **documented non-nav allowlist exception, NOT migrated**. `action_utils.rs:652` is TEST (inside `mod tests` at `:437`) → excluded, nothing to migrate | 6/7 |
| **NIT** guard path drift | Corrected to `tests/g_misc0/critical_rules_have_guards.rs` everywhere; fix stale `CLAUDE.md` pointer | 6/7 |
| **NIT** ≤2-line drifts | Corrected to tree-verified `fn` anchors (re-verified against the live tree; **fn-head lines are stable anchors re-verified at impl time**): `find_export_span` `fn` `:1920` (default heuristic body `:1944-1954`, `return Some((0,0))` `:1954`), static-split `:504-564`, `merge_*` `fn` lines (`resolve_vue_tsx_range:647` with trailing `.unwrap_or_default()` `:681`/`merge_definitions_with_barrel_resolver:725` with else-arm `Range::default()` `:786`/`merge_references:874`/`merge_rename_locations:943`/`merge_code_actions:1102`), `tsx_range_to_vue_range:138`, `normalize_vue_path:829`/`normalize_vue_path_owned:849`, `try_component_contract_definition:375`, `try_barrel_export_definition:584`, `definition_at_position:41`, `resolved_import_definition:418` (`Range::default()` `:422`), `ide_context_by_path:1380-1389`, props emit `fn`s (`process_v_html:1024`/`process_v_text:1040`/`process_v_model:826`/`resolve_prefixed_expr:1279`/`resolve_prefixed_dynamic_arg:1293`). **`:786` is attributed to `merge_definitions_with_barrel_resolver` (NOT `resolve_vue_tsx_range`, whose own default is `:681`).** | all |
| **OQ-1** | location-only `TsNavigationBackend`/`TsgoNavigationBackend`→`TsDefinitionSpan`; 4 guards; no new `SemanticQueryKey` mode | 5, 6/7 |
| **OQ-2** | `emit.rs`: `EmitText` + typed `EmitOp` (+`MoveOriginal`/`anchor`) + `emit_jsx_binding_value` + `JsxBindingValue`; migrate ALL callers then delete | 2 |
| **OQ-3** | anchor at `IndexedReady`; non-optional `TemplateAnalysisState{NoTemplate,Ready}`; name-span field; FileStart only for empty; discriminating guard | 3 |
| **OQ-4** | `CompileSnapshotId(u128) = Hash128(compile warm-hit identity)` (semantic_hash + override hashes + profile + parser/compiler/version + canonicalized `ReadSetSignature` facts, per §3.1.2 — TSX-byte hash is debug-metadata-only) at publish on `CachedTsx`/`IdeResponse`/tsgo registry; on every `TsDefinitionSpan`/`CompilerLocation`; non-IDE via `TargetSourceContext.source_hash`; mismatch⇒DROP | 4, 5, 6/7 |
| **OQ-5** | nav core in `verter_session::navigation::definition` (LSP-free; `CanonicalId`=`Arc<str>`); engine owns `classify` + Vue→TSX mapping (P1-3); `verter_lsp` = typed-coords + `TsgoNavigationBackend` + render only; `host_*_context`; ban-LSP-types + classification-in-engine guards | 5 |
| **OQ-6** | Rust engine tests = hard gate; e2e mandatory non-skippable committed fixtures + ban-`this.skip()` guard | 5, 6/7 |
| **OQ-7** | type-definition via `ProjectSemanticDispatch` in a dedicated `verter_session::navigation::type_definition` module (P2-A guard allowlist); shared rendering; `CompilerLocation` carries intrinsic coordinate-space+provenance (`Generated{snapshot_id}`/`RealSource{source_hash}`, P1-1) — provenance-less location unrepresentable + drop-on-mismatch guard | 6/7 |

### Tracked carry-forwards (explicit, justified — NOT P0/P1)
- **CF-1 (from P1-H; corrected per P1-4 + round-3 P2-B):** `Range::default()` in a **non-navigation** LSP PRODUCTION surface that warrants a dedicated follow-up — **`features/workspace_symbol.rs`** (4 production sites at `:47`/`:67`/`:92`/`:114`, all before the `#[cfg(test)]` module at `:160`). **Reason:** workspace-symbol results are not go-to-definition navigation results; they are a different feature family with their own correctness model (symbol-index ranges) and would expand this plan beyond its charter. The `ban_range_default_in_nav_construction` guard allowlists this file (so no nav regression hides behind it); this plan documents it for a dedicated follow-up that applies the same strict-mapper/snapshot discipline. **The guard's allowlist has TWO documented non-nav exceptions total: `workspace_symbol.rs` (this CF-1 follow-up) and `extract_component.rs:118` (CF-2 below — a correct (0,0) new-file content insertion, not a deferred defect).**
  - **Round-3 P2-B correction (verified against the live tree):** `features/call_hierarchy.rs` is **NOT** in CF-1 and is **NOT** allowlisted. Its only `Range::default()` occurrences (`:290`/`:291`) are **inside** `#[cfg(test)] mod tests` (the test module starts at `:219`, so `:290`/`:291` come AFTER it). There is **zero** production `Range::default()` in `call_hierarchy.rs`; the standard test-module exemption covers `:290`/`:291`, so the file needs no allowlist entry. (The earlier "2 production sites at `:290`/`:291`, before the `#[cfg(test)]` module at `:219`" phrasing was self-contradictory — `:290`/`:291` are after `:219`, hence test, not production — and is removed.)
  - **P1-4 correction (verified against the live tree):** the round-1 CF-1 list was wrong on two counts. (1) `merge.rs:1136`/`:1142` are NOT hover/highlight sites — they are the `.vue.d.ts`/`.vue.ts` (`:1136`) and external `.ts/.js` (`:1142`) **code-action** `Range::default()` fallbacks inside `merge_code_actions` (`:1102-1162`); they are go-to-target navigation defaults that the binding architecture + §6/7-C + P2-E REQUIRE **deleting**, so they are removed from CF-1 and live in the §6/7-C migration/deletion set (NOT allowlisted). `merge_document_highlights` (`fn` at `:1018`, body `:1018-1056`) has **no** `Range::default()` at all (the cited "highlight" site does not exist — it maps via `tsx_range_to_vue_range`). (2) `merge.rs:1647`/`:1705`/`:1785`/`:1820`/`:2023`/`:3060` are ALL inside `#[cfg(test)] mod tests` (which starts at `merge.rs:1318`), so they are already covered by the standard "except explicit tests" guard exemption and are removed from the production CF-1 enumeration. After this correction **the merge.rs PRODUCTION `Range::default()` carry-forward set is EMPTY** — every production site in merge.rs (`:681` `resolve_vue_tsx_range`'s trailing `.unwrap_or_default()`, `:786` `merge_definitions_with_barrel_resolver`'s else-arm default, `:913`/`:921` `merge_references`, `:989`/`:996` `merge_rename_locations`, `:1136`/`:1142` `merge_code_actions`) is deleted by §6/7. No real non-nav hover/tokens/inlay PRODUCTION `Range::default()` site exists in the LSP (verified: no such file carries one outside test modules), so none is carried forward.
  - **Round-3 P2-B correction (verified against the live tree):** `action_utils.rs:652` is **NOT** in the §6/7-C migration set — it is **inside** `#[cfg(test)] mod tests` (the test module starts at `action_utils.rs:437`, so `:652` comes AFTER it; it is TEST, not production, and there is nothing to migrate). There is **no** genuine PRODUCTION code-action nav-result `Range::default()` to migrate: the only remaining production code-action defaults are `merge_code_actions:1136`/`:1142`, which are deleted with that function in §6/7.
  - **Round-3 P2-2 correction (verified against the live tree): `extract_component.rs:118` is a NON-NAV allowlist exception, NOT a migration.** It is the `range` of a `TextEdit` that writes the FULL CONTENT of a brand-new `.vue` file — `extract_component_action` builds a `CreateFile { new_file_uri }` immediately followed by a `TextDocumentEdit` targeting that new uri with `TextEdit { range: Range::default(), new_text: new_component_source }` (the source comment reads "Write content to the new file"). For a not-yet-existent empty file, `Range::default()` (0,0) is the CORRECT, intentional insertion point. It is a REFACTOR_EXTRACT code action edit, **not** a go-to-definition navigation result, and **not** in the merge.rs wrong-0:0 defect class. It sits before that file's `#[cfg(test)]` module at `:155` (so it is production), and is therefore added to the `ban_range_default_in_nav_construction` allowlist (or excluded from that guard's file scope) with the new-file-insert justification — **NOT migrated**. **CF-2 (extract_component.rs:118):** a documented non-nav `Range::default()` in PRODUCTION whose (0,0) is semantically correct for new-file content insertion; allowlisted, not a defect.

### Repo discoveries that changed scope
- The combined-TSX cache value is **`CachedTsx`** (`types.rs:1723`), built at `virtual_file_pipeline.rs:1851` — this is the concrete OQ-4 "compile-cache publish point". `CompileSnapshotId` is added here (and mirrored to `IdeResponse`), not on a separate compile-cache entry type. The struct flows through `CompileOutputValue.tsx: Option<CachedTsx>` (`cache_runtime/compile_output_node.rs:130`) and is read back via `CompileOutputNodeFactValidatedSession::peek_tsx` (`cache_runtime/compile_output_node.rs:576`), so the added `CompileSnapshotId` propagates through the cache-runtime node for free; direct `compile_slots` access is forbidden.
- The canonical-id type is **`CanonicalId = Arc<str>`** (`capture_token.rs:64`), not `CanonicalFileId`. All nav-core types use `CanonicalId`.
- `IndexedReady` (`project_type_store.rs:95-164`; `whole_hash` is the first field at `:115`, not the struct start) is the single canonical post-parse artifact carrying `snapshot`, `script_analysis`, `cached_parse`, `eval_source` — the OQ-3 anchor publication site (built at `prepared_decl.rs:1708` + `overlay_materialize.rs:533`).
- `FileAnalysisSnapshot.template` is `Option<...>` (`types.rs:1244`), confirming OQ-3's hazard; the new `TemplateAnalysisState` makes the Vue path non-optional.
- `AnalyzedMacro` (`types.rs:1306-1365`) has no name-prop span — confirms P1-F needs a new producer field.
- The single typed-IR dispatch entry is `execute_cooperative` (`semantic_query_memo/mod.rs:2197`); the OQ-1 guard-4 runtime "zero typed-IR dispatch" assertion counts via the per-request dispatch counter `record_dispatch`/`record_dispatch_warm` (`:2359`/`:2416`/`:2353`, via `with_active_capture`) or the `AuditObserver` counter (NIT-2). `verter_audit::current_observer()` at `:3818` is a doc-comment/prose mention only, not the counter.
- The R6 meta-guard is at **`tests/g_misc0/critical_rules_have_guards.rs`** (not `tests/critical_rules_have_guards.rs`); `architecture_guards.rs` is at `tests/architecture_guards.rs`.
- **(P1-A) Verified virtual→source canonical mechanism.** There is **no** reverse `.vue.tsx`→`.vue` registry in the host. The reverse mapping is **path-suffix stripping**: `normalize_vue_path` (`fn` at `crates/verter_lsp/src/tsgo/merge.rs:829`) / `normalize_vue_path_owned` (`fn` at `:849`) strips `.vue.tsx`/`.vue.jsx` (4 chars), `.vue.ts` (3 chars), `.vue.d.ts` (`.d.ts`), guarded by a `vue_source_exists` predicate (`nav_features.rs:807-808` = `|p| host.get_source(p).is_some()`). The host owns the FORWARD direction only (`virtual_file_pipeline.rs:1440` derives the component/virtual TSX from the `.vue` canonical; `list_virtual_files`/`list_virtual_nodes` at `:1346`). The tsgo synced-file registry (`project_sync.rs`) only forward-syncs `.vue.tsx` content and holds the per-file `CompileSnapshotId` (Phase 4B); it has no reverse map. Because `merge.rs` is deleted in §6/7 and the nav core must be LSP-free (OQ-5), the strip+existence-check moves to a host/engine helper (`owner_canonical_for_generated`), and `TsDefinitionSpan::GeneratedTsx` carries the OWNER `.vue` canonical explicitly (P1-A) so normalize/render/dedup never operate on a `.vue.tsx` key.
