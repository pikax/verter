# Verter go-to-definition — BINDING ARCHITECTURE (codex final decision, 2026-06-02)

Source: final codex consult, informed by 4 code/empirical subagents + prior-art research + 2 architecture consults. This is the binding architecture the implementation plan must follow. Breaking changes allowed. Four NEVERS: no shims, no legacy/dual paths, no stubs, no shortcuts.

## DECISION
KEEP the V3 source-map substrate. Do NOT replace it with a second authoritative per-range mapping table. Root cause is 4 IDE-only emit sites collapsing prefix+identifier+suffix into one overwrite — not a V3 incapability. The existing chunk model already produces correct navigation on the common paths. End state: one chunk graph, one V3 map, strict lookup, typed coordinates, one definition engine.

## A. MAPPING SUBSTRATE
`CodeTransform` remains authoritative; V3 remains the serialized map the LSP consumes. Make the emit API typed so invalid mapped overwrites are impossible for IDE codegen:

```rust
enum EmitOp {
    InsertUnmapped { at: SourceByteOffset, text: String },
    InsertMapped { at: SourceByteOffset, text: String, source_start: SourceByteOffset, content_offset: GeneratedByteLen },
    PreserveOriginal { source: SourceByteRange },
    OverwriteSyntheticBoundary { source: SourceByteRange, text: String, anchor: SourceByteOffset },
    MoveOriginal { source: SourceByteRange, at: SourceByteOffset },
}
```

Rules:
- Inserted compiler text is unmapped.
- Original source remains 1:1, preferably per-byte/hires for navigable user text.
- Prefixing an identifier = InsertUnmapped(prefix) + preserved identifier, OR InsertMapped with mapped content beginning at content_offset.
- Overwrite allowed ONLY for synthetic boundaries and semantic anchors, NEVER for `prefix + user_identifier`.
- No mapped-overwrite primitive is added.
- IDE-only helpers produce structured pieces `{ prefix, source_expr_span, suffix, occurrences }`, not flat mapped strings.
- Runtime helpers (`build_prefixed_expr`, `resolve_simple_expr`, `resolve_prefix/suffix`, `collect_binding_patches`) keep their existing flat-string contract where runtime output depends on it.

v-model = multiple mapped occurrences (NOT one formatted replacement). Each use of the original expression gets its own emission: emit_unmapped(syntax) + emit_mapped_expr(original_span, generated_text, content_offset) + emit_unmapped(suffix). 3 occurrences → 3 separate mapped generated ranges → same source span. All assignment/call/temp/punctuation text is synthetic/unmapped.

PositionMapper contract (permanent):
- tsx_to_vue → Option<SourceRange>; vue_to_tsx → Option<GeneratedRange>.
- Strict interval lookup only. NO nearest-previous token interpolation. NO column-delta extrapolation through overwritten/synthetic text.
- Generated position inside unmapped inserted/synthetic content → None.
- Half-open ranges map only when both endpoints are inside compatible mapped spans.
- Typed coordinate wrappers mandatory at boundaries: SourceByteOffset, GeneratedByteOffset, SourceUtf16Offset, GeneratedUtf16Offset, LspPosition, TsPosition.
- Round-trip invariant for exact mapped user text: source→generated→source == original (modulo accepted half-open boundary clamping).
- Failed mapping = feature result drop, NOT 0:0.

Capability semantics without replacing V3: navigation uses only exact source-map mappings (preserved original text or explicit InsertedMapped). Synthetic anchors are named compiler-provided anchors only, not free-form reverse mappings. Rename/reference/linking semantics live as feature metadata on the typed IR, not hidden in source-map interpolation.

## B. SINGLE DefinitionEngine
Delete the three-path design. One engine pipeline:
LSP position → classify into DefinitionQuery → map into generated TSX if needed → ask tsgo backend when semantic TS resolution required → normalize generated/declaration/native result into DefinitionTarget → terminalize barrels and Vue default exports → render target into LSP Location using the target file's own snapshot → exact dedup.

```rust
enum DefinitionTarget {
    RealSource { uri: CanonicalUri, span: SourceByteRange, symbol: SymbolKind, snapshot: SnapshotId },
    SfcComponent { uri: CanonicalUri, anchor: SfcComponentAnchor, snapshot: SnapshotId },
    ExternalDeclaration { uri: CanonicalUri, span: SourceByteRange, symbol: SymbolKind, snapshot: SnapshotId },
}
struct SfcComponentAnchor {
    preferred_span: SourceByteRange,
    kind: DefineOptionsName | ExplicitExportDefault | ScriptSetupStart | TemplateRootStart | FileStart,
}
```

SfcComponentAnchor is first-class compiler/analysis output on every `.vue` record. Fixed priority:
1. defineOptions({ name }) name span.
2. Explicit `<script>` export default expression/object span.
3. `<script setup>` tag start.
4. First template root tag start.
5. FileStart only for truly empty SFCs, recorded explicitly, never a silent fallback.

Target mappers/spans come from the HOST, not the open-doc registry. For every target `.vue`: ensure_compiled(canonical_uri, profile) then read host.get_ide(...).source_map, compiled TSX path, analysis, export graph, SnapshotId. A mapper is usable only when its snapshot matches the TSX snapshot tsgo used. Stale mapper → drop target.

Per-target behavior:
- Same-file binding → RealSource, SFC-absolute span, live LineIndex.
- `.vue` component import/tag → SfcComponent via recorded anchor.
- script-setup → anchor = `<script setup>` tag start unless defineOptions({name}) gives a better name span.
- explicit export default → anchor = export-default expr/object span.
- template-only → anchor = first template root tag start.
- named `.vue` export → named binding's real source span.
- barrel terminal → follow to terminal; `export { default as Foo } from './Foo.vue'` → that component's SfcComponentAnchor.
- `.ts`/`.js` export → RealSource via target file's LineIndex, never a Vue mapper.
- library `.vue.d.ts`/`.d.ts` → ExternalDeclaration at the real `.d.ts` declaration span; no fabricated `.vue` target.

Dedup by canonical identity (uri, target kind, normalized source span, symbol identity), NOT suffix preference. Canonical source beats generated TSX for the same symbol. `.d.ts` kept only when it's the real terminal declaration or no real source target exists. `.vue` never dropped merely because a non-`.vue` result exists.

## LEGACY DELETIONS (delete outright)
- find_export_span default-export first-binding/first-macro/(0,0) heuristic.
- resolved_import_definition returning Range::default().
- cross-file early return in handle_goto_definition that skips tsgo when native returns any scalar.
- resolve_vue_tsx_range fallback to the current file's mapper.
- `.vue.d.ts`/`.vue.ts` Range::default() handling in merge.
- "prefer non-.vue" suffix-based dedup.
- virtual-file cross-file Range::default() branches in definition, type definition, references.
- external `.ts/.js` references/rename/code-action Range::default() fallbacks.
- contract-prop final fallback pushing Range::default().
- IDE-only prefixed-string producers resolve_prefixed_expr / resolve_prefixed_dynamic_arg as flat mapped strings.
- any silent conversion from unmappable generated offsets to nearest source position.

## INVARIANTS + TESTS
Mapping: common paths exact; v-html/v-text/`:[key]`/native v-model map identifiers exactly; v-model verifies every repeated generated occurrence maps back; generated prefix positions (_ctx./$setup.) → None; overwritten punctuation interiors → None unless querying the explicit anchor; UTF-16/CRLF/emoji/astral/tabs/multiline round-trip; strict half-open boundary at start/end/one-past-end; static guard: no IDE codegen call passes format!("{}{}", prefix, ident) into a mapped overwrite.
Definition: `.vue` default import → component anchor not first binding; script-setup/explicit-export-default/template-only/defineOptions each land on expected anchor; barrel default terminal → terminal `.vue` anchor; named `.vue` export → named binding; `.ts/.js` export → real target line via target LineIndex; `.vue.d.ts` stays `.d.ts`; missing target compile → no definition not 0:0; stale mapper snapshot rejected; dedup keeps canonical `.vue`; virtual `.vue.tsx` editing uses the same DefinitionEngine.
Architecture guards: ban Range::default() in navigation result construction (except explicit tests); ban current-file mapper fallback for cross-file targets; require SnapshotId equality before mapping generated target ranges; require every `.vue` analysis record to contain SfcComponentAnchor; require all definition surfaces (definition, type definition, references, rename, code actions) to call DefinitionEngine.

## MIGRATION SHAPE (ordered, each independently landable + verifiable)
1. Harden PositionMapper: typed coordinates, strict interval lookup, None on unmapped, no extrapolation. Blast radius: mapping callers handle Option.
2. Replace IDE-only prefixed expression emission with structured emit helpers. Add regression tests for the 4 confirmed desync constructs + v-model repeated occurrences.
3. Add SfcComponentAnchor to compiler/analysis output and remove find_export_span.
4. Implement host-sourced target mapper loading with ensure_compiled + snapshot identity checks.
5. Introduce DefinitionTarget + DefinitionEngine; route same-file native, tsgo, barrels, declarations, Vue components through it.
6. Delete old merge arbitration, suffix preference, current-file fallback, virtual-file special cases, all navigation Range::default() fallbacks.
7. Extend the same target rendering path to type definition, references, rename, code actions so cross-file behavior is shared, not forked.
