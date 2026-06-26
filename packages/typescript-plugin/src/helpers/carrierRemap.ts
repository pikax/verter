import { SourceMap } from "node:module";
import type { CarrierStoreReader } from "./carrierStore";
import { cleanupCarrierVirtualImportPath, containingFileAwareExists, normalizePath } from "./utils";

/**
 * Navigation-span remapping over the store-published carrier source maps.
 *
 * A carrier companion (`Comp.vue.tsx`) is generated TypeScript whose offsets do
 * not line up with the `.vue`/`.svelte` source. A definition / hover / completion
 * result landing inside a companion must be mapped back to the source position
 * the user actually sees. The map is the V3 source map the Rust LSP publishes to
 * the store (`maps/blake3-<map_hash>.json`, the serialized `CodeTransform`
 * source-map / `ProviderPositionMapper`); this module reads it through the store
 * reader (NOT a NAPI compile) and applies the same offset→origin remapping.
 *
 * The parsed `SourceMap` objects are cached by `map_hash` (content-addressed, so
 * a hash hit is always the same map) and only re-read when the carrier's
 * `map_hash` changes.
 */

/** A text span in offset form (the shape TS `TextSpan` carries). */
export interface OffsetSpan {
  start: number;
  length: number;
}

/** A remapped span: the source file it maps to plus the source-side span. */
export interface RemappedSpan {
  fileName: string;
  textSpan: OffsetSpan;
}

const parsedMapCache = new Map<string, SourceMap | null>();

function offsetToLineColumn(text: string, offset: number): { line: number; column: number } {
  const prefix = text.slice(0, offset);
  const lines = prefix.split("\n");
  const lastLine = lines.length > 0 ? lines[lines.length - 1] : "";
  return {
    line: lines.length,
    column: lastLine.length + 1,
  };
}

function lineColumnToOffset(text: string, line: number, column: number): number | null {
  if (line < 1 || column < 1) return null;
  const lines = text.split("\n");
  if (line > lines.length) return null;

  let offset = 0;
  for (let i = 0; i < line - 1; i += 1) {
    offset += lines[i].length + 1;
  }
  const lineText = lines[line - 1];
  if (column - 1 > lineText.length) return null;
  return offset + column - 1;
}

/**
 * Parse (and cache by `map_hash`) the V3 source map for a ready carrier. A
 * missing/unparseable map caches `null` so a repeated lookup does not re-read.
 */
function parsedMapFor(
  reader: CarrierStoreReader,
  mapHash: string,
  mapRel: string,
): SourceMap | null {
  const cached = parsedMapCache.get(mapHash);
  if (cached !== undefined) {
    return cached;
  }
  const raw = reader.readMapSync(mapRel);
  let map: SourceMap | null = null;
  if (raw !== undefined && raw !== null) {
    try {
      map = new SourceMap(raw as ConstructorParameters<typeof SourceMap>[0]);
    } catch {
      map = null;
    }
  }
  parsedMapCache.set(mapHash, map);
  return map;
}

/**
 * The original source text the published V3 map carries in `sourcesContent` for
 * `sourceFileName` (matched against the map's `sources`, forward-slash
 * normalized), or `undefined` when the map carries no content for it. This is
 * the EXACT source the map's mappings were produced against — the authority for
 * the inverse line/column→offset conversion when the host has no readable copy
 * of the carrier source (an in-memory `.vue`/`.svelte` not on disk).
 */
function sourceContentFor(map: SourceMap, sourceFileName: string): string | undefined {
  const payload = map.payload as
    | { sources?: readonly string[]; sourcesContent?: readonly (string | null)[] }
    | undefined;
  const sources = payload?.sources;
  const sourcesContent = payload?.sourcesContent;
  if (!sources || !sourcesContent) {
    return undefined;
  }
  const want = normalizePath(sourceFileName);
  for (let i = 0; i < sources.length; i += 1) {
    if (normalizePath(sources[i]) === want) {
      const content = sourcesContent[i];
      return typeof content === "string" ? content : undefined;
    }
  }
  return undefined;
}

/**
 * Remap a span inside the carrier companion `providerPath` back to its source
 * position, using the carrier's published source map. Returns `null` when the
 * companion is not ready, carries no map, or the span has no source origin —
 * the caller then falls back to its non-mapped path (fail closed, never a
 * mis-mapped span).
 *
 * `readCompanionText` reads the companion's generated text (the blob) — the
 * offset→line/column conversion is over the generated text the offsets index;
 * `readSourceText` reads the original `.vue`/`.svelte` source for the inverse
 * line/column→offset conversion.
 */
export function remapCarrierSpan(
  reader: CarrierStoreReader,
  providerPath: string,
  span: OffsetSpan,
  readCompanionText: (providerPath: string) => string | undefined,
  readSourceText: (sourcePath: string) => string | undefined,
): RemappedSpan | null {
  const ready = reader.readyFile(providerPath);
  if (!ready || ready.map_rel === undefined) {
    return null;
  }
  const map = parsedMapFor(reader, ready.map_hash, ready.map_rel);
  if (!map) {
    return null;
  }
  const companionText = readCompanionText(providerPath);
  if (companionText === undefined) {
    return null;
  }

  const { line, column } = offsetToLineColumn(companionText, span.start);
  const origin = map.findOrigin(line, column);
  if (!("fileName" in origin) || !origin.fileName) {
    return null;
  }

  const originalFileName = normalizePath(origin.fileName);
  // The carrier SOURCE text for the inverse line/column→offset conversion. Read
  // it from the host first (the on-disk / in-memory `.vue`/`.svelte`), then fall
  // back to the published map's `sourcesContent` — the EXACT source bytes the
  // map's mappings were produced against. The fallback is load-bearing for a
  // carrier opened in-memory whose source is not on disk in the tsserver process
  // (the LSP holds it): without it every carrier-edit remap fails closed and the
  // whole response (rename / remove-unused / references) is dropped.
  const originalText = readSourceText(originalFileName) ?? sourceContentFor(map, originalFileName);
  if (originalText === undefined) {
    return null;
  }

  const originalStart = lineColumnToOffset(originalText, origin.lineNumber, origin.columnNumber);
  if (originalStart === null) {
    return null;
  }

  // Map the span END through the same source map so a multi-character carrier
  // span (a rename target, a reference highlight) maps to its FAITHFUL source
  // length — a hardcoded length-1 collapsed every span to a single character,
  // so a rename/highlight on a `.vue` identifier always under-selected. The end
  // is the offset one-past the span's last code unit (`span.start + span.length`).
  const mappedLength = mapSpanLength(
    map,
    companionText,
    originalText,
    span,
    originalStart,
    originalFileName,
  );

  return {
    fileName: originalFileName,
    textSpan: { start: originalStart, length: mappedLength },
  };
}

/**
 * The faithful SOURCE length for a carrier span whose start maps to
 * `originalStart` in `originalFileName`.
 *
 * The span END (`span.start + span.length`) is mapped through the SAME map:
 * - When it maps to a LATER offset in the SAME source file, that delta is the
 *   faithful source length (the multi-character identifier / expression case —
 *   the codegen emits a distinct mapping segment at the chunk boundary).
 * - Otherwise (the end maps to the same-or-earlier source offset because the
 *   map lacks a finer end segment, or maps into a DIFFERENT source region) the
 *   carrier span's OWN length is the best faithful approximation: a preserved
 *   identifier/expression chunk is byte-identical in carrier and source, so its
 *   length carries over. This is never the old hardcoded `1`; a real
 *   multi-character token keeps its width either way.
 *
 * A zero-length input span maps to a zero-length source span (a caret position).
 */
function mapSpanLength(
  map: SourceMap,
  companionText: string,
  originalText: string,
  span: OffsetSpan,
  originalStart: number,
  originalFileName: string,
): number {
  if (span.length === 0) {
    return 0;
  }
  const endPos = offsetToLineColumn(companionText, span.start + span.length);
  const endOrigin = map.findOrigin(endPos.line, endPos.column);
  if (
    "fileName" in endOrigin &&
    endOrigin.fileName &&
    normalizePath(endOrigin.fileName) === originalFileName
  ) {
    const originalEnd = lineColumnToOffset(
      originalText,
      endOrigin.lineNumber,
      endOrigin.columnNumber,
    );
    if (originalEnd !== null && originalEnd > originalStart) {
      return originalEnd - originalStart;
    }
  }
  return span.length;
}

/** Test/maintenance hook: drop the parsed-map cache. */
export function clearCarrierMapCache(): void {
  parsedMapCache.clear();
}

// ── companion→source RESPONSE remapping ────────────────────────────────────
//
// A provider response (definition / references / rename / code-action /
// completion-detail) can carry a carrier COMPANION path (`Comp.vue.tsx` /
// `Comp.svelte.tsx` / `Comp.vue.verter.ts`) plus a span/edit measured against
// the GENERATED companion text. Such a response must be mapped back to the
// `.vue`/`.svelte` SOURCE — both its `fileName` AND its span/edit offsets —
// before it reaches the editor. A span/edit that does not map to a source
// origin (a generated-only helper region) FAILS CLOSED: the entry is dropped,
// never returned with a companion path or a mis-mapped source span. Spans in
// the user's real `.ts`/`.js` files (non-companion paths) pass through
// unchanged. These mappers are the response-side mirror of the host-side
// carrier serving — a single shared `remapCarrierSpan` is the only offset
// translator, so the response surface never diverges from it.

/**
 * The read callbacks + reader a response mapper needs to translate a companion
 * span back to source through the published carrier source map. `readCompanion`
 * reads the GENERATED companion text the response offsets index;
 * `readSource` reads the original `.vue`/`.svelte` text for the inverse
 * line/column→offset conversion. `fileExists` is the host existence predicate
 * the inserted-import-specifier rewrite uses to disambiguate a Svelte rune path
 * from a carrier companion (Vue strips by shape, never needing it).
 */
export interface CarrierRemapContext {
  reader: CarrierStoreReader;
  readCompanion: (providerPath: string) => string | undefined;
  readSource: (sourcePath: string) => string | undefined;
  fileExists?: (candidate: string) => boolean;
}

/**
 * Whether `fileName` is a carrier COMPANION provider path the store owns (a
 * `.vue.tsx` / `.svelte.tsx` / `.vue.verter.ts` companion), as opposed to a
 * user's real `.ts`/`.js` file or a `.vue`/`.svelte` SOURCE path. A response
 * entry whose path is a companion is the only one that needs its `fileName`
 * rewritten to source + its span remapped; every other path passes through.
 *
 * The classification is the store's authority, NOT a suffix sniff: a path the
 * project owns as a companion (it is a `ready_files` entry OR an `owned_sources`
 * provider URI) is a companion. This matches the host-side `carrierContent` /
 * `carrierExists` ownership checks, so the response surface and the served
 * surface agree on what a companion is.
 */
export function isCarrierCompanionPath(reader: CarrierStoreReader, fileName: string): boolean {
  if (reader.readyFile(fileName)) {
    return true;
  }
  const owned = reader.ownedSourceFor(fileName);
  // `ownedSourceFor` also matches a SOURCE path (`source_uri`); a companion is
  // specifically the `provider_uri`. A bare source path is NOT a companion (it
  // is already the target we map TO), so require the matched provider identity.
  return owned !== undefined && normalizePath(owned.provider_uri) === normalizePath(fileName);
}

/**
 * The carrier SOURCE path (`Comp.vue` / `Comp.svelte`) that a companion
 * `provider` path backs, resolved through the store's owned-source table — the
 * SAME authority the host hooks serve from. Returns `undefined` when `provider`
 * is not a known companion (so the caller fails closed rather than fabricating a
 * source path). This is the store's classification, NOT a suffix sniff.
 */
export function sourceForCarrierCompanion(
  reader: CarrierStoreReader,
  provider: string,
): string | undefined {
  const owned = reader.ownedSourceFor(provider);
  if (owned === undefined || normalizePath(owned.provider_uri) !== normalizePath(provider)) {
    return undefined;
  }
  return normalizePath(owned.source_uri);
}

/**
 * Whether a definition-like response targets the WHOLE MODULE / a carrier's
 * file-level identity (the import-target / default-export case) rather than a
 * specific token inside the generated companion.
 *
 * The signal is the TYPED `kind` TypeScript stamps on the response:
 * - `ts.ScriptElementKind.moduleElement` (`"module"`) — what
 *   `getModuleSpecifierNavigationResult` mints for a `./Comp.vue` import
 *   specifier, and what TS returns for go-to-def on a module reference;
 * - `scriptElement` (`"script"`) — the whole-`SourceFile` element;
 * - `alias` (`ts.ScriptElementKind.alias`) — the definition of an imported
 *   binding. For `import Comp from "./Comp.vue"`, the resolved declaration is
 *   the component carrier's synthesized default-export re-export, which TS
 *   reports as an `alias`. A companion `alias` definition is therefore the
 *   carrier's module-level identity (the component itself), not a specific
 *   user-authored token.
 *
 * This is the ONLY reliable discriminator: a span anchored at offset 0 is NOT
 * sufficient, because a token-level response (a `ReferenceEntry` /
 * `RenameLocation` carrying no `kind`) can legitimately land in a generated-only
 * region at offset 0, which must FAIL CLOSED — not be reinterpreted as a
 * file-level navigation. A response with a concrete-declaration `kind`
 * (`const`/`function`/`class`/…) is a specific token. This is a structural read
 * of the typed response, NOT a path/string sniff. The check only ever runs when
 * the precise span FAILED to map, so a mappable specific token is never coerced
 * here.
 */
export function isModuleLevelDefinition(def: { kind?: string; textSpan: OffsetSpan }): boolean {
  return def.kind === "module" || def.kind === "script" || def.kind === "alias";
}

/**
 * Remap a MODULE-LEVEL companion definition (an import-specifier / default-export
 * navigation whose target is the carrier AS A FILE) back to the `.vue`/`.svelte`
 * SOURCE: `fileName` → the source path, `textSpan`/`contextSpan` → the source
 * FILE START (`{ start: 0, length: 0 }`). The correct navigation target for "go
 * to the imported `.vue`" is the source file itself; its companion offset-0
 * region has no specific source mapping, so a source-file-start caret is both
 * correct and self-consistent (the user lands in the `.vue`, never in a
 * non-existent `.vue.tsx`).
 *
 * Fail-closed: returns `undefined` when the source path for `companion` cannot
 * be determined (the caller then drops the entry rather than surface the
 * companion path). Mutates + returns the input on success.
 */
export function remapModuleLevelCompanionToSource<
  T extends {
    fileName: string;
    textSpan: OffsetSpan;
    contextSpan?: OffsetSpan;
    originalTextSpan?: OffsetSpan;
    originalContextSpan?: OffsetSpan;
    originalFileName?: string;
  },
>(reader: CarrierStoreReader, def: T): T | undefined {
  const source = sourceForCarrierCompanion(reader, def.fileName);
  if (source === undefined) {
    return undefined;
  }
  def.fileName = source;
  def.textSpan = { start: 0, length: 0 };
  if (def.contextSpan) {
    def.contextSpan = { start: 0, length: 0 };
  }
  // `originalFileName`/`originalTextSpan` describe a PRIOR `.d.ts.map` redirect,
  // not a carrier companion — drop the stale generated originals.
  if (def.originalFileName !== undefined) {
    delete def.originalFileName;
  }
  if (def.originalTextSpan !== undefined) {
    delete def.originalTextSpan;
  }
  if (def.originalContextSpan !== undefined) {
    delete def.originalContextSpan;
  }
  return def;
}

/**
 * Map a `DocumentSpan`-shaped object (`DefinitionInfo` / `ReferenceEntry` /
 * `RenameLocation` / `ImplementationLocation`) from a carrier companion back to
 * the `.vue`/`.svelte` SOURCE.
 *
 * - `fileName` is NOT a companion → the object passes through UNCHANGED (a span
 *   in the user's real `.ts`/`.js` is already source-correct).
 * - `fileName` IS a companion and its `textSpan` maps to a source origin → the
 *   returned object's `fileName` is the source path and `textSpan`/`contextSpan`
 *   are the remapped source spans.
 * - `fileName` IS a companion but the span does NOT map (a generated-only
 *   region) → `undefined` (FAIL CLOSED — the caller drops the entry; a source
 *   path is NEVER paired with a generated offset, a companion path is NEVER
 *   surfaced to the editor).
 *
 * Mutates+returns the input on a successful remap (the response objects are
 * the engine's own, mutated in place exactly as the definition path already
 * does) — the caller treats the return value as the authority.
 */
export function remapDocumentSpan<
  T extends {
    fileName: string;
    textSpan: OffsetSpan;
    contextSpan?: OffsetSpan;
    originalTextSpan?: OffsetSpan;
    originalContextSpan?: OffsetSpan;
    originalFileName?: string;
    kind?: string;
  },
>(ctx: CarrierRemapContext, span: T): T | undefined {
  const companion = span.fileName;
  if (!isCarrierCompanionPath(ctx.reader, companion)) {
    return span;
  }
  const remapped = remapCarrierSpan(
    ctx.reader,
    companion,
    span.textSpan,
    ctx.readCompanion,
    ctx.readSource,
  );
  if (!remapped) {
    // A MODULE-LEVEL companion target (the file's module identity — e.g. a
    // find-all-references hit on a component's default export, whose `textSpan`
    // is the module start that has no specific source mapping) navigates to the
    // `.vue`/`.svelte` SOURCE FILE, not the non-existent companion. A
    // SPECIFIC-token companion span that does not map is a generated-only region
    // and still fails closed (dropped).
    if (isModuleLevelDefinition(span)) {
      return remapModuleLevelCompanionToSource(ctx.reader, span);
    }
    return undefined;
  }
  // The context span (the enclosing-declaration span) is remapped through the
  // ORIGINAL companion path/offsets — BEFORE `fileName` is reassigned — so it is
  // not collapsed to the name span; if it cannot be mapped it falls back to the
  // name span rather than a stale generated offset.
  if (span.contextSpan) {
    const remappedContext = remapCarrierSpan(
      ctx.reader,
      companion,
      span.contextSpan,
      ctx.readCompanion,
      ctx.readSource,
    );
    span.contextSpan = remappedContext ? remappedContext.textSpan : remapped.textSpan;
  }
  span.fileName = remapped.fileName;
  span.textSpan = remapped.textSpan;
  // `originalFileName`/`originalTextSpan` describe a PRIOR remap (a `.d.ts.map`
  // redirect). A carrier companion is not such a redirect target; drop the
  // stale originals so no consumer reads a generated offset back out of them.
  if (span.originalFileName !== undefined) {
    delete span.originalFileName;
  }
  if (span.originalTextSpan !== undefined) {
    delete span.originalTextSpan;
  }
  if (span.originalContextSpan !== undefined) {
    delete span.originalContextSpan;
  }
  return span;
}

/**
 * Remap an array of `DocumentSpan`-shaped entries, DROPPING any companion entry
 * whose span could not be mapped (fail closed). Non-companion entries (the
 * user's real `.ts` spans) and successfully-remapped companion entries are
 * retained in order. Returns a NEW array (the input is not mutated in length).
 */
export function remapDocumentSpans<
  T extends {
    fileName: string;
    textSpan: OffsetSpan;
    contextSpan?: OffsetSpan;
    originalTextSpan?: OffsetSpan;
    originalContextSpan?: OffsetSpan;
    originalFileName?: string;
  },
>(ctx: CarrierRemapContext, spans: readonly T[]): T[] {
  const out: T[] = [];
  for (const span of spans) {
    const mapped = remapDocumentSpan(ctx, span);
    if (mapped !== undefined) {
      out.push(mapped);
    }
  }
  return out;
}

/**
 * Like [`remapDocumentSpans`], but for the references of a `ReferencedSymbol`
 * whose DEFINITION resolved to a carrier SOURCE as the carrier's module-level
 * identity (`moduleSource` is that source path). A reference that lands on the
 * SAME carrier companion as the moved definition but whose precise span cannot
 * be mapped (the carrier's synthesized default-export re-export has no faithful
 * per-token source) is KEPT on the companion path instead of being dropped — so
 * find-all-references still REACHES the component (the upstream LSP maps a
 * carrier-companion reference back to source; the bare-source path with a
 * file-start span is rejected by tsserver's own reference post-processing, so
 * the companion path is the correct hand-off identity here). A reference in a
 * DIFFERENT companion, or a normal mappable companion reference, follows the
 * standard [`remapDocumentSpan`] path (map-or-drop). When `moduleSource` is
 * `undefined` this is exactly [`remapDocumentSpans`] (every unmappable companion
 * reference fails closed).
 */
function remapDocumentSpansWithModuleFallback<
  T extends {
    fileName: string;
    textSpan: OffsetSpan;
    contextSpan?: OffsetSpan;
    originalTextSpan?: OffsetSpan;
    originalContextSpan?: OffsetSpan;
    originalFileName?: string;
  },
>(ctx: CarrierRemapContext, spans: readonly T[], moduleSource: string | undefined): T[] {
  const out: T[] = [];
  for (const span of spans) {
    const isMovedCompanion =
      moduleSource !== undefined &&
      isCarrierCompanionPath(ctx.reader, span.fileName) &&
      sourceForCarrierCompanion(ctx.reader, span.fileName) === moduleSource;
    const mapped = remapDocumentSpan(ctx, span);
    if (mapped !== undefined) {
      out.push(mapped);
    } else if (isMovedCompanion) {
      // The reference is the carrier's own module-level identity (same companion
      // as the moved definition) but has no faithful per-token mapping → keep it
      // on the companion path so the component stays REACHABLE. Dropping it would
      // make the component unreachable from find-all-references.
      out.push(span);
    }
  }
  return out;
}

/**
 * Remap a `ReferencedSymbol` (the `findReferences` grouping): its `definition`
 * is a `DocumentSpan` and its `references` are `DocumentSpan` entries. The
 * symbol is DROPPED entirely when its `definition` is a companion whose span
 * cannot be mapped (fail closed — a symbol grouping anchored on an unmappable
 * generated definition must not surface). Otherwise the definition is remapped
 * (or passed through) and each reference is remapped, dropping any unmappable
 * companion reference.
 */
export function remapReferencedSymbol<
  R extends {
    fileName: string;
    textSpan: OffsetSpan;
    contextSpan?: OffsetSpan;
    originalTextSpan?: OffsetSpan;
    originalContextSpan?: OffsetSpan;
    originalFileName?: string;
  },
  S extends {
    definition: {
      fileName: string;
      textSpan: OffsetSpan;
      contextSpan?: OffsetSpan;
      originalTextSpan?: OffsetSpan;
      originalContextSpan?: OffsetSpan;
      originalFileName?: string;
    };
    references: R[];
  },
>(ctx: CarrierRemapContext, symbol: S): S | undefined {
  // Capture whether the definition started on a carrier companion BEFORE the
  // remap mutates `symbol.definition` in place.
  const defStartedOnCompanion = isCarrierCompanionPath(ctx.reader, symbol.definition.fileName);
  const definition = remapDocumentSpan(ctx, symbol.definition);
  if (definition === undefined) {
    return undefined;
  }
  // When the definition resolved to a carrier SOURCE (the symbol IS a carrier's
  // module-level / default-export identity — the component itself), references
  // that land on the SAME carrier companion are the carrier's own module
  // identity too: map them to that source rather than dropping them. This keeps
  // find-all-references REACHING the component even when the carrier's
  // synthesized re-export has no faithful per-token source mapping.
  const movedToSource =
    defStartedOnCompanion && !isCarrierCompanionPath(ctx.reader, definition.fileName);
  symbol.definition = definition;
  symbol.references = remapDocumentSpansWithModuleFallback(
    ctx,
    symbol.references,
    movedToSource ? definition.fileName : undefined,
  );
  return symbol;
}

/**
 * Remap a `FileTextChanges` (a code-action / refactor / rename file edit) from a
 * carrier companion back to the `.vue`/`.svelte` SOURCE.
 *
 * - `fileName` is NOT a companion → passes through UNCHANGED (an edit to a
 *   user's real `.ts` is already source-correct), BUT each `TextChange.newText`
 *   still gets the inserted-specifier rewrite (a companion specifier inside an
 *   added import in a real `.ts` → bare `.vue`/`.svelte`).
 * - `fileName` IS a companion → `fileName` is rewritten to the source path and
 *   EACH `TextChange.span` is remapped through the source map. A change whose
 *   span cannot be mapped is DROPPED (fail closed — a generated-region edit is
 *   never applied to the source). If EVERY change is dropped the whole
 *   `FileTextChanges` is dropped (`undefined`) so no empty source edit is
 *   emitted.
 *
 * The single shared source path that backs the companion is resolved ONCE via
 * the source map (the first mappable change), so every retained change lands in
 * the same source file.
 */
export function remapFileTextChanges<
  C extends { span: OffsetSpan; newText: string },
  F extends { fileName: string; textChanges: readonly C[] },
>(ctx: CarrierRemapContext, change: F): F | undefined {
  if (!isCarrierCompanionPath(ctx.reader, change.fileName)) {
    // A real-file edit: only the inserted-import specifier inside `newText`
    // needs the companion→bare rewrite; offsets are already source-correct.
    const rewritten: C[] = change.textChanges.map((tc) => ({
      ...tc,
      newText: rewriteInsertedSpecifier(ctx, tc.newText, change.fileName),
    }));
    return { ...change, textChanges: rewritten };
  }

  const companion = change.fileName;
  let sourceFileName: string | undefined;
  const remappedChanges: C[] = [];
  for (const tc of change.textChanges) {
    const remapped = remapCarrierSpan(
      ctx.reader,
      companion,
      tc.span,
      ctx.readCompanion,
      ctx.readSource,
    );
    if (!remapped) {
      // Generated-only edit region → DROP (never apply to the source).
      continue;
    }
    sourceFileName ??= remapped.fileName;
    remappedChanges.push({
      ...tc,
      span: remapped.textSpan,
      newText: rewriteInsertedSpecifier(ctx, tc.newText, remapped.fileName),
    });
  }
  if (sourceFileName === undefined || remappedChanges.length === 0) {
    // No change mapped → drop the whole file edit (fail closed).
    return undefined;
  }
  return { ...change, fileName: sourceFileName, textChanges: remappedChanges };
}

/**
 * Remap an array of `FileTextChanges`, dropping any companion file edit whose
 * changes are all unmappable (fail closed). Returns a NEW array.
 */
export function remapAllFileTextChanges<
  C extends { span: OffsetSpan; newText: string },
  F extends { fileName: string; textChanges: readonly C[] },
>(ctx: CarrierRemapContext, changes: readonly F[]): F[] {
  const out: F[] = [];
  for (const change of changes) {
    const mapped = remapFileTextChanges(ctx, change);
    if (mapped !== undefined) {
      out.push(mapped);
    }
  }
  return out;
}

/**
 * Rewrite a carrier-companion import specifier inside an engine-produced
 * `newText` (an auto-import / add-missing-import edit) back to the bare
 * `.vue`/`.svelte` specifier: `from "./Comp.vue.tsx"` / `from "./Comp.vue.verter.ts"`
 * → `from "./Comp.vue"`. Delegates to the shared
 * [`cleanupCarrierVirtualImportPath`] (the single carrier-suffix-strip
 * authority, which covers the `.tsx` IDE companion + the `.verter.ts` API
 * carrier + the `.d.ts` alias), so the specifier rewrite never diverges from
 * the display-path cleanup. An AMBIGUOUS Svelte rune specifier
 * (`./store.svelte.ts` with no `./store.svelte` carrier) is left intact by the
 * backing-file check, so a real rune import is never mangled.
 *
 * This is a TEXT cleanup over the already-engine-produced edit string (a
 * specifier IS text), NOT a semantic decision — the architecture's typed-IR
 * rule governs semantic classification, not literal import-path normalization.
 */
export function rewriteInsertedSpecifier(
  ctx: CarrierRemapContext,
  newText: string,
  containingFile: string,
): string {
  const existsRel = relativeAwareExists(ctx, containingFile);
  return cleanupCarrierVirtualImportPath(newText, existsRel);
}

/**
 * The existence predicate the specifier rewrite passes to
 * [`cleanupCarrierVirtualImportPath`] so a RELATIVE backing candidate (a
 * `./Comp.svelte` reconstructed from a `./Comp.svelte.ts` token) resolves
 * against the containing file's directory. Returns `undefined` when the context
 * carries no host predicate — the unambiguous Vue strip still works by shape;
 * an ambiguous Svelte suffix is then left intact (never mangled). Delegates to
 * the shared [`containingFileAwareExists`] so the relative-resolution discipline
 * matches the completion/quick-fix display-path cleanup exactly.
 */
function relativeAwareExists(
  ctx: CarrierRemapContext,
  containingFile: string,
): ((candidate: string) => boolean) | undefined {
  if (ctx.fileExists === undefined) {
    return undefined;
  }
  return containingFileAwareExists(ctx.fileExists, containingFile);
}
