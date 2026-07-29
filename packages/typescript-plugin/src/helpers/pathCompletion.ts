import type tsModule from "typescript/lib/tsserverlibrary";
import {
  CARRIER_SOURCE_EXTENSIONS,
  normalizePath,
  resolveCarrierImportTarget,
  type CarrierImportOwnershipReader,
  type OwnedSource,
} from "@verter/language-shared";

/**
 * Carrier import-path completion for module-specifier string positions.
 *
 * TypeScript's own path completion lists only its supported TS/JS extensions
 * (plus ambient `*.x` wildcard modules) — `getSupportedExtensionsForModuleResolution`
 * never consults `extraFileExtensions` — so a plain `.ts` buffer typing
 * `import X from './` is never offered `./Comp.vue` / `./Comp.svelte`, even
 * though the plugin's `resolveModuleNameLiterals` override resolves exactly
 * that specifier. This module supplies the missing entries.
 *
 * Fail-closed by construction: candidates are enumerated from ONE carrier
 * store manifest snapshot (the same authority the module resolver consults),
 * and each one must pass [`resolveCarrierImportTarget`] — the exact "which
 * surface does an ordinary import resolve to" policy — PLUS the non-blocking
 * readiness arms that policy deliberately leaves to its caller. The guarantee,
 * stated precisely: every OFFERED entry inserts a bare authored specifier
 * (`./Comp.vue`) that resolves against the manifest snapshot the offer was
 * computed from — its import surface is either published (`ready_files`,
 * role `CarrierApi`) or retained as last-good content. The converse does not
 * hold: an owned carrier in its publication WARM-UP window (owned row, no
 * ready entry, no last-good) is WITHHELD even though accepting it later could
 * still resolve through the bounded cold read — blocking per candidate on a
 * keystroke path is not acceptable, and offering without blocking would offer
 * specifiers that do not (yet) resolve. Concretely:
 *  - a conflicted/ambiguous carrier (Rust left it out of the manifest) is
 *    never offered;
 *  - an IDE-role-only carrier (import surface not owned → the resolver
 *    abstains) is never offered;
 *  - an owned carrier whose import surface has not published yet and has no
 *    last-good content is never offered (the warm-up window);
 *  - a Svelte rune module (self-file import surface, not a component carrier)
 *    is never offered as a carrier;
 *  - the offered NAME is always the authored basename — never a companion /
 *    virtual path.
 *
 * Cost: no filesystem walk and no directory listing, but NOT I/O-free — the
 * request performs exactly ONE manifest read (a `statSync` change check, plus
 * a re-parse only when the manifest actually changed) via
 * [`CarrierPathCompletionReader.importCompletionSnapshot`], then every
 * per-candidate check (ownership policy, readiness, last-good) runs against
 * that in-memory snapshot. The bound is per-request, independent of how many
 * carriers the directory holds, and only applies after the position has been
 * proven to sit inside a relative module specifier.
 */

/**
 * Everything one completion request consults, taken from a SINGLE manifest
 * read: the reader-scoped owned rows plus the canonical provider paths of the
 * ready `CarrierApi` surfaces in that same manifest.
 */
export interface CarrierImportCompletionSnapshot {
  /** The reader-scoped `owned_sources` rows, in manifest order. */
  readonly ownedSources: readonly OwnedSource[];
  /**
   * Canonical provider paths (the reader's `canonicalPath` spelling) of every
   * `ready_files` entry with role `CarrierApi` in the same manifest.
   */
  readonly readyApiProviders: ReadonlySet<string>;
}

/** The reader slice carrier path completion consults. */
export interface CarrierPathCompletionReader {
  /** The host filesystem identity policy (see [`CarrierStoreReader.canonicalPath`]). */
  canonicalPath?(fileName: string): string;
  /** ONE manifest read yielding everything a completion request consults. */
  importCompletionSnapshot(): CarrierImportCompletionSnapshot;
  /** In-memory last-good content lookup — never a manifest read. */
  lastGoodBlobFor(providerPath: string): string | undefined;
}

/**
 * The deepest node whose `[fullStart, end)` range contains `position`.
 * Public-API descent (`forEachChild`), no internal `getTokenAtPosition`.
 */
function deepestNodeAt(
  ts: typeof tsModule,
  sourceFile: tsModule.SourceFile,
  position: number,
): tsModule.Node {
  let node: tsModule.Node = sourceFile;
  for (;;) {
    const child: tsModule.Node | undefined = ts.forEachChild(node, (candidate) =>
      candidate.getFullStart() <= position && position < candidate.getEnd() ? candidate : undefined,
    );
    if (child === undefined) return node;
    node = child;
  }
}

/**
 * The string literal that is a MODULE SPECIFIER containing `position` (caret
 * strictly after the opening quote), or `undefined`. Covers the standard
 * specifier positions: `import ... from "x"`, `export ... from "x"`,
 * `import("x")`, `require("x")`, `import x = require("x")`, and the
 * import-type node `typeof import("x")`.
 */
export function moduleSpecifierLiteralAt(
  ts: typeof tsModule,
  sourceFile: tsModule.SourceFile,
  position: number,
): tsModule.StringLiteralLike | undefined {
  const node = deepestNodeAt(ts, sourceFile, position);
  if (!ts.isStringLiteralLike(node)) return undefined;
  // Inside the literal means after the opening quote (mirrors `isInString`).
  if (position <= node.getStart(sourceFile)) return undefined;

  const parent = node.parent;
  if (parent === undefined) return undefined;
  if (
    (ts.isImportDeclaration(parent) || ts.isExportDeclaration(parent)) &&
    parent.moduleSpecifier === node
  ) {
    return node;
  }
  if (ts.isExternalModuleReference(parent) && parent.expression === node) {
    return node;
  }
  if (
    ts.isCallExpression(parent) &&
    parent.arguments[0] === node &&
    (parent.expression.kind === ts.SyntaxKind.ImportKeyword ||
      (ts.isIdentifier(parent.expression) && parent.expression.text === "require"))
  ) {
    return node;
  }
  if (
    ts.isLiteralTypeNode(parent) &&
    parent.parent !== undefined &&
    ts.isImportTypeNode(parent.parent) &&
    parent.parent.argument === parent
  ) {
    return node;
  }
  return undefined;
}

/** `path` up to (excluding) the last `/`; `""` when there is no parent. */
function directoryOf(path: string): string {
  const normalized = normalizePath(path);
  const index = normalized.lastIndexOf("/");
  return index < 0 ? "" : normalized.slice(0, index);
}

/**
 * Resolve `fragmentDirectory` (a `./` / `../` prefix of a specifier, up to and
 * including its last `/`) against the importing file's directory into a
 * normalized absolute directory. Pure string resolution — no filesystem.
 */
function resolveFragmentDirectory(containingDirectory: string, fragmentDirectory: string): string {
  const segments =
    `${normalizePath(containingDirectory)}/${normalizePath(fragmentDirectory)}`.split("/");
  const out: string[] = [];
  for (const [index, segment] of segments.entries()) {
    if (segment === "" && index > 0) continue; // collapse `//`, drop trailing `/`
    if (segment === ".") continue;
    if (segment === "..") {
      // Never pop past a posix root (`""` head) or a windows drive head.
      if (out.length > 1 || (out.length === 1 && out[0] !== "" && !out[0].endsWith(":"))) {
        out.pop();
      }
      continue;
    }
    out.push(segment);
  }
  return out.join("/");
}

/**
 * The RAW characters of a string-literal module specifier as they appear in
 * the source file: everything between the quotes — opening quote excluded,
 * closing quote excluded when the literal is terminated. This is DISTINCT
 * from the cooked `literal.text`: TypeScript collapses escape sequences there
 * (`.\\W.sv` in source cooks to `.\W.sv`), so cooked offsets do not address
 * source characters and a replacement span must never be computed from them.
 */
export function rawModuleSpecifierText(
  sourceText: string,
  literalStart: number,
  literalEnd: number,
): string {
  const terminated =
    literalEnd - literalStart >= 2 &&
    sourceText.charCodeAt(literalEnd - 1) === sourceText.charCodeAt(literalStart);
  return sourceText.slice(literalStart + 1, terminated ? literalEnd - 1 : literalEnd);
}

/**
 * The RAW offset just past the last directory separator in `rawText`, scanning
 * escape-aware left to right: a bare `/`, an escaped `\\` (a cooked
 * backslash — the Windows-style separator), and an escaped `\/` (a cooked
 * forward slash) all separate; any OTHER escape sequence is skipped whole so
 * its payload character is never misread as a separator.
 */
function rawBasenameStart(rawText: string): number {
  let start = 0;
  for (let index = 0; index < rawText.length; index += 1) {
    const ch = rawText[index];
    if (ch === "/") {
      start = index + 1;
      continue;
    }
    if (ch === "\\") {
      const next = rawText[index + 1];
      if (next === "\\" || next === "/") {
        start = index + 2;
      }
      index += 1; // consume the escaped character either way
    }
  }
  return start;
}

/**
 * Mirror of TypeScript's `getDirectoryFragmentTextSpan`, computed over the
 * RAW source characters: the replacement span covering the BASENAME portion
 * of the typed fragment.
 *  - `span` — the raw-offset span an accept must replace;
 *  - `wordPrefix` — the basename is empty or plain identifier text; the
 *    client's word-based prefix replacement covers it, no span needed;
 *  - `unrepresentable` — the raw and cooked basenames disagree beyond the
 *    handled separator escapes (`\\`, `\/`): no trustworthy raw span exists,
 *    and offering with a wrong (or no) span corrupts on accept — the caller
 *    must offer nothing.
 */
type FragmentSpanResult =
  | { readonly kind: "span"; readonly span: tsModule.TextSpan }
  | { readonly kind: "wordPrefix" }
  | { readonly kind: "unrepresentable" };

function fragmentReplacementSpan(
  cookedText: string,
  rawText: string,
  literalStart: number,
): FragmentSpanResult {
  const rawStart = rawBasenameStart(rawText);
  const rawBasename = rawText.slice(rawStart);
  const cookedIndex = Math.max(cookedText.lastIndexOf("/"), cookedText.lastIndexOf("\\"));
  const cookedBasename = cookedText.slice(cookedIndex + 1);
  // The one raw↔cooked agreement this module supports: past the last
  // separator, the raw characters ARE the cooked characters. An escape inside
  // the basename, or a separator escape the scan does not model, breaks that
  // and fails closed.
  if (rawBasename !== cookedBasename) {
    return { kind: "unrepresentable" };
  }
  if (rawBasename.length === 0 || /^[a-zA-Z_$][a-zA-Z0-9_$]*$/.test(rawBasename)) {
    return { kind: "wordPrefix" };
  }
  return {
    kind: "span",
    span: { start: literalStart + 1 + rawStart, length: rawBasename.length },
  };
}

/** Matches TypeScript's `isPathRelativeToScript` (`./x`, `../x`, `.\x`). */
function isRelativeFragment(fragment: string): boolean {
  return /^\.\.?[\\/]/.test(fragment);
}

export interface CarrierPathCompletionInput {
  /** The file the completion request ran in (normalized or native spelling). */
  readonly containingFile: string;
  /** The module-specifier literal's cooked text (`node.text`). */
  readonly literalText: string;
  /**
   * The literal's RAW source characters between the quotes (see
   * [`rawModuleSpecifierText`]). Replacement spans are computed from THESE
   * offsets — never from the cooked text, whose collapsed escapes desync it
   * from the file.
   */
  readonly literalRawText: string;
  /** Offset of the literal's opening quote in the file. */
  readonly literalStart: number;
  /** The project-scoped carrier store reader (manifest authority). */
  readonly reader: CarrierPathCompletionReader;
  /** Entry names TypeScript already offered (dedupe guard). */
  readonly existingNames: ReadonlySet<string>;
}

/**
 * The carrier completion entries for one module-specifier position: every
 * manifest-owned component carrier whose source sits in the directory the
 * typed relative fragment names AND whose accepted specifier resolves against
 * this same manifest snapshot (ownership policy + ready-or-last-good — the
 * module header states the exact guarantee). Empty for non-relative fragments
 * (package specifiers are out of this path's scope).
 */
export function carrierPathCompletionEntries(
  input: CarrierPathCompletionInput,
): tsModule.CompletionEntry[] {
  const fragment = normalizePath(input.literalText);
  if (!isRelativeFragment(fragment)) return [];

  const spanResult = fragmentReplacementSpan(
    input.literalText,
    input.literalRawText,
    input.literalStart,
  );
  // No trustworthy raw replacement span — an offer would corrupt on accept.
  if (spanResult.kind === "unrepresentable") return [];
  const replacementSpan = spanResult.kind === "span" ? spanResult.span : undefined;

  const fragmentDirectory = fragment.slice(0, fragment.lastIndexOf("/") + 1);
  const baseDirectory = resolveFragmentDirectory(
    directoryOf(input.containingFile),
    fragmentDirectory,
  );
  const identity = (path: string): string =>
    input.reader.canonicalPath?.(path) ?? normalizePath(path);
  const baseIdentity = identity(baseDirectory);

  // ONE manifest read per request; every per-candidate check below runs
  // against this snapshot, in memory.
  const snapshot = input.reader.importCompletionSnapshot();
  // The snapshot-backed ownership view [`resolveCarrierImportTarget`] queries:
  // first row matching either the source or the provider spelling, under the
  // host identity policy — the disk reader's `ownedSourceFor` semantics,
  // without its per-call manifest read.
  const ownedByIdentity = new Map<string, OwnedSource>();
  for (const owned of snapshot.ownedSources) {
    for (const key of [identity(owned.source_uri), identity(owned.provider_uri)]) {
      if (!ownedByIdentity.has(key)) ownedByIdentity.set(key, owned);
    }
  }
  const ownership: CarrierImportOwnershipReader = {
    canonicalPath: (path: string) => identity(path),
    ownedSourceFor: (path: string) => ownedByIdentity.get(identity(path)),
  };

  const entries: tsModule.CompletionEntry[] = [];
  const offered = new Set<string>();
  for (const owned of snapshot.ownedSources) {
    if (owned.role !== "CarrierApi") continue;
    const source = normalizePath(owned.source_uri);
    if (identity(directoryOf(source)) !== baseIdentity) continue;
    // The exact policy an accepted import resolves through. Conflicted
    // (manifest-absent), rune modules, and identity mismatches abstain here.
    const target = resolveCarrierImportTarget(ownership, source);
    if (target.kind !== "resolve") continue;
    // The non-blocking readiness arms of actual resolution: a published
    // import surface in this same snapshot, or retained last-good content.
    // An owned-but-unpublished carrier (warm-up) is withheld.
    if (
      !snapshot.readyApiProviders.has(identity(target.provider)) &&
      input.reader.lastGoodBlobFor(target.provider) === undefined
    ) {
      continue;
    }
    const name = source.slice(source.lastIndexOf("/") + 1);
    if (input.existingNames.has(name) || offered.has(name)) continue;
    offered.add(name);
    entries.push({
      name,
      kind: "script" as tsModule.ScriptElementKind.scriptElement,
      // The descriptor-owned carrier extension (`.vue` / `.svelte`), the same
      // shape TypeScript's own path entries use (`kindModifiersFromExtension`).
      kindModifiers: CARRIER_SOURCE_EXTENSIONS.find((extension) => name.endsWith(extension)) ?? "",
      // TypeScript's `SortText.LocationPriority` — sorts with sibling path entries.
      sortText: "11",
      ...(replacementSpan === undefined ? {} : { replacementSpan }),
    });
  }
  return entries;
}
