import path from "node:path";
import { VIRTUAL_FILE_NAMING, type VirtualPathPolicy } from "../generated/virtual-file-naming";

/**
 * The fixed suffix a `VirtualPathPolicy` appends to form a DISTINCT virtual
 * file (the component-carrier dual-file model). A `selfFile`/`none` policy (the
 * standalone rune-module model — the file serves its own path) has no distinct
 * suffix, so it returns `null` and contributes no carrier virtual file.
 */
function policySuffix(policy: VirtualPathPolicy): string | null {
  return policy.kind === "suffix" ? policy.suffix : null;
}

// The carrier virtual-file naming is DERIVED from the generated, byte-pinned
// `virtual-file-naming.ts` mirror of the Rust framework-adapter descriptor
// column (the single authority). The four former Vue-only regex literals
// (`/\.vue$/`, `/\.vue\.ts$/`, `/\.vue\.d\.ts$/`, `/\.vue\.__verter_test\.ts$/`)
// are RETIRED — every carrier's extension + virtual suffixes come from the
// column, so adding a carrier (e.g. `.svelte`) needs no edit here.

const RELATIVE_REGEXP = /^\.\.?($|[\\/])/;

export type VuePublicApiMode = "public" | "testing";

/** Escape a literal for use inside a `RegExp`. */
function escapeRegExp(literal: string): string {
  return literal.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

interface CarrierNaming {
  /** The carrier file extension (`.vue` / `.svelte`). */
  readonly extension: string;
  /** Matches the bare carrier file (`*.vue`). */
  readonly carrier: RegExp;
  /** Matches the API virtual file (`*.vue.ts`), if the row ships one. */
  readonly apiTs: RegExp | null;
  /** Matches the `.d.ts` accepted-spelling alias (`*.vue.d.ts`) — uniform. */
  readonly dTs: RegExp;
  /** Matches the testing-API virtual file (`*.vue.__verter_test.ts`). */
  readonly testingTs: RegExp | null;
  /** The API suffix (`.ts`), if the row ships one. */
  readonly apiSuffix: string | null;
  /** The testing-API suffix (`.__verter_test.ts`), if the row ships one. */
  readonly testingApiSuffix: string | null;
}

/**
 * The per-carrier naming table, built once from the generated column. Each
 * carrier extension maps to its bare-carrier + virtual-suffix regexes.
 */
const CARRIERS: readonly CarrierNaming[] = Object.values(VIRTUAL_FILE_NAMING)
  // A true COMPONENT carrier projects a DISTINCT import-surface virtual file
  // (a `suffix` policy → `*.vue.ts`). A standalone rune module (`selfFile`
  // import surface) serves its OWN path — it is NOT a carrier virtual file, so
  // it must not enter the carrier naming table.
  .filter((row) => row.carrierExtension !== null && policySuffix(row.importSurface) !== null)
  .map((row) => {
    const ext = row.carrierExtension as string;
    const e = escapeRegExp(ext);
    const apiSuffix = policySuffix(row.importSurface);
    const testingApiSuffix = row.testingApiSuffix;
    return {
      extension: ext,
      carrier: new RegExp(`${e}$`),
      apiTs: apiSuffix ? new RegExp(`${e}${escapeRegExp(apiSuffix)}$`) : null,
      // The `.d.ts` accepted-spelling alias is `{carrier_ext}.d.ts` uniformly.
      dTs: new RegExp(`${e}${escapeRegExp(".d.ts")}$`),
      testingTs: testingApiSuffix ? new RegExp(`${e}${escapeRegExp(testingApiSuffix)}$`) : null,
      apiSuffix,
      testingApiSuffix,
    };
  });

/**
 * The set of REAL standalone-module extensions a manifest row owns via a
 * `selfFile` import surface (e.g. Svelte's `.svelte.ts` / `.svelte.js` rune
 * modules — a file that serves its OWN path, NOT a generated carrier virtual).
 * A row contributes its `carrierExtension` here iff its import surface is
 * `selfFile`. This is the manifest-derived authority that makes a component
 * carrier's `{ext}.ts` virtual suffix AMBIGUOUS: when `{ext}.ts` is also a real
 * self-file extension (`.svelte` + `.ts` == `.svelte.ts`), a `X.{ext}.ts` path
 * may be either the virtual API of component `X.{ext}` OR a real standalone
 * module — and stripping it to `X.{ext}` corrupts the real module.
 */
const SELF_FILE_MODULE_EXTENSIONS: readonly string[] = Object.values(VIRTUAL_FILE_NAMING)
  .filter((row) => row.carrierExtension !== null && row.importSurface.kind === "selfFile")
  .map((row) => row.carrierExtension as string);

/**
 * Whether a component carrier's virtual suffix (e.g. `.svelte` + `.ts`,
 * `.svelte` + `.d.ts`) is AMBIGUOUS against a real self-file module family. A
 * carrier extension is ambiguous when it is the leading SEGMENT of a registered
 * self-file module extension — i.e. some self-file extension starts with
 * `{carrierExt}.` (Svelte's rune family `.svelte.ts` makes `.svelte` ambiguous,
 * so EVERY `.svelte`-rooted virtual — `.svelte.ts`, `.svelte.d.ts` — is
 * ambiguous, because `X.svelte.ts` / `X.svelte.d.ts` may belong to a real
 * `X.svelte.ts` rune module rather than a `X.svelte` component). Such a path
 * cannot be classified as virtual purely by SHAPE — it needs a backing-file
 * check (the `X{carrierExt}` carrier source must exist). Vue's `.vue` is NOT the
 * stem of any self-file extension, so `.vue.*` virtuals are unambiguous and
 * strip by shape alone.
 */
function virtualSuffixIsAmbiguous(carrierExt: string, _virtualSuffix: string): boolean {
  const stem = `${carrierExt}.`;
  return SELF_FILE_MODULE_EXTENSIONS.some((ext) => ext.startsWith(stem));
}

const isRelative = (fileName: string) => RELATIVE_REGEXP.test(fileName);

export function normalizePath(fileName: string): string {
  return fileName.replace(/\\/g, "/");
}

/**
 * The IDE-carrier companion suffix a `VirtualPathPolicy` produces — the
 * `ide` column, NOT the import surface. This is the path the Rust LSP
 * publishes a carrier's interactive content at (`Comp.vue.tsx` /
 * `Comp.svelte.tsx`) and the in-project module-resolution redirect target:
 *   - `suffix`        → its single suffix (Svelte's IDE policy → `.tsx`).
 *   - `jsxConditional`→ the `nonJsx` suffix (the TypeScript-TSX carrier the
 *     host publishes for a TS carrier — Vue's `.tsx`); the `jsx` (`.jsx`)
 *     branch is the JavaScript-carrier form and is not the published TS
 *     identity the redirect targets.
 *   - `selfFile`/`none` → no distinct companion (returns `null`).
 *
 * Derived from the generated column so adding a carrier needs no edit here.
 */
function ideCarrierSuffix(policy: VirtualPathPolicy): string | null {
  switch (policy.kind) {
    case "suffix":
      return policy.suffix;
    case "jsxConditional":
      return policy.nonJsx;
    case "selfFile":
    case "none":
      return null;
  }
}

/**
 * The per-carrier IDE-companion naming table: each component carrier extension
 * (`.vue`/`.svelte`) mapped to its IDE-carrier suffix (the `ide` column). Built
 * once from the generated column, in parallel to the import-surface `CARRIERS`
 * table above.
 */
const IDE_CARRIERS: readonly { extension: string; ideSuffix: string }[] = Object.values(
  VIRTUAL_FILE_NAMING,
)
  .filter((row) => row.carrierExtension !== null && ideCarrierSuffix(row.ide) !== null)
  .map((row) => ({
    extension: row.carrierExtension as string,
    ideSuffix: ideCarrierSuffix(row.ide) as string,
  }));

/**
 * The IDE-carrier companion path for a bare carrier file (`Comp.vue` →
 * `Comp.vue.tsx`, `Comp.svelte` → `Comp.svelte.tsx`), or `null` when `fileName`
 * is not a recognised component carrier. This is the in-project module-resolution
 * redirect target — the SOURCE carrier identity both engines resolve a bare
 * `./Comp.vue` import to — distinct from the `.verter.ts` API carrier (the
 * cross-package/project-ref redirect target).
 */
export function toIdeCarrierFileName(fileName: string): string | null {
  const normalized = normalizePath(fileName);
  for (const c of IDE_CARRIERS) {
    if (normalized.endsWith(c.extension)) {
      return `${normalized}${c.ideSuffix}`;
    }
  }
  return null;
}

/**
 * The component-carrier SOURCE extensions (`.vue`, `.svelte`) — the file
 * extensions tsserver must be told to accept as program members via
 * `extraFileExtensions` so a `getExternalFiles`-advertised carrier SOURCE path
 * enters the configured project's Program. Derived from the generated column (a
 * row with a carrier extension + a distinct IDE companion is a true component
 * carrier), so a new framework participates with no edit here.
 */
export const CARRIER_SOURCE_EXTENSIONS: readonly string[] = IDE_CARRIERS.map((c) => c.extension);

/**
 * Whether `fileName` is a bare component-carrier SOURCE path (`*.vue` / `*.svelte`)
 * — the path tsserver queries after `getExternalFiles` advertises it. Distinct
 * from a companion virtual path (`*.vue.tsx` / `*.vue.verter.ts`).
 */
export function isCarrierSourcePath(fileName: string): boolean {
  const normalized = normalizePath(fileName);
  return CARRIER_SOURCE_EXTENSIONS.some(
    (ext) => normalized.endsWith(ext) && toIdeCarrierFileName(normalized) !== null,
  );
}

/**
 * The IDE-companion provider path that backs a carrier SOURCE path's content
 * when tsserver makes the source a program member, or `null` for a non-carrier
 * path. `getExternalFiles` advertises the SOURCE path (`Comp.vue`); tsserver then
 * asks the host for the source path's snapshot/kind/version, which the plugin
 * answers with the IDE companion's (`Comp.vue.tsx`) carrier content + TSX kind.
 * This is exactly [`toIdeCarrierFileName`] (source → IDE companion).
 */
export function carrierSourceToCompanion(sourcePath: string): string | null {
  return isCarrierSourcePath(sourcePath) ? toIdeCarrierFileName(sourcePath) : null;
}

/** The carrier row whose bare extension matches `fileName`, if any. */
function carrierFor(fileName: string): CarrierNaming | undefined {
  return CARRIERS.find((c) => c.carrier.test(fileName));
}

export function getVueVirtualFileInfo(
  fileName: string,
): { sourceFileName: string; mode: VuePublicApiMode } | null {
  const normalized = normalizePath(fileName);

  for (const c of CARRIERS) {
    if (c.testingTs?.test(normalized) && c.testingApiSuffix) {
      return {
        sourceFileName: normalized.slice(0, -c.testingApiSuffix.length),
        mode: "testing",
      };
    }
  }

  for (const c of CARRIERS) {
    if (c.dTs.test(normalized)) {
      return {
        sourceFileName: normalized.slice(0, -".d.ts".length),
        mode: "public",
      };
    }
  }

  for (const c of CARRIERS) {
    if (c.apiTs?.test(normalized) && c.apiSuffix) {
      return {
        sourceFileName: normalized.slice(0, -c.apiSuffix.length),
        mode: "public",
      };
    }
  }

  return null;
}

export function toVueVirtualFileName(fileName: string, mode: VuePublicApiMode): string {
  const normalized = normalizePath(fileName);
  const c = carrierFor(normalized);
  if (mode === "testing" && c?.testingApiSuffix) {
    return `${normalized}${c.testingApiSuffix}`;
  }
  const apiSuffix = c?.apiSuffix ?? ".ts";
  return `${normalized}${apiSuffix}`;
}

export function stripVueVirtualSuffix(fileName: string): string {
  return getVueVirtualFileInfo(fileName)?.sourceFileName ?? normalizePath(fileName);
}

/**
 * Backing-file-aware carrier virtual-suffix strip for consumers that RESOLVE or
 * UPSERT a path (where mis-stripping corrupts classification), e.g. macro-type
 * hydration's `resolveModule`/`upsert`. Unlike the pure-shape
 * [`stripVueVirtualSuffix`], this strips `X.{ext}.ts` → `X.{ext}` ONLY when the
 * backing carrier source `X.{ext}` actually exists, so a real standalone rune
 * module (`store.svelte.ts` with no `store.svelte`) is left UNCHANGED rather
 * than corrupted into a phantom component path.
 *
 * `fileExists` is the host's existence predicate (the TS language-service
 * host's `fileExists`). A path with no virtual shape, or an ambiguous virtual
 * shape whose backing carrier does not exist, returns the plain normalised path.
 */
export function stripVueVirtualSuffixBackingAware(
  fileName: string,
  fileExists: (candidate: string) => boolean,
): string {
  const info = getVueVirtualFileInfo(fileName);
  if (info && fileExists(info.sourceFileName)) {
    return info.sourceFileName;
  }
  return normalizePath(fileName);
}

/**
 * The carrier virtual-suffix strip rules, derived ONCE from the carrier naming
 * table, longest-suffix-first so `*.vue.__verter_test.ts` and `*.vue.d.ts` win
 * over `*.vue.ts` (and every carrier — `.vue`, `.svelte` — is covered). Each
 * rule maps a carrier virtual-file suffix (e.g. `.vue.ts`) embedded anywhere in
 * a string back to the bare carrier extension (`.vue`).
 */
const CARRIER_VIRTUAL_SUFFIX_STRIPPERS: readonly {
  pattern: RegExp;
  carrierExt: string;
  virtualSuffix: string;
  /**
   * Whether `{carrierExt}{virtualSuffix}` collides with a real self-file module
   * extension (Svelte's `.svelte.ts`). An ambiguous suffix only strips when a
   * backing-file check proves the path is virtual; an unambiguous one (Vue)
   * strips by shape.
   */
  ambiguous: boolean;
}[] = CARRIERS.flatMap((c) => {
  const suffixes: string[] = [];
  if (c.testingApiSuffix) suffixes.push(c.testingApiSuffix);
  // The `.d.ts` accepted-spelling alias is `{carrier_ext}.d.ts` uniformly.
  suffixes.push(".d.ts");
  if (c.apiSuffix) suffixes.push(c.apiSuffix);
  // The IDE-carrier companion suffix (the `ide` column — `Comp.vue.tsx`). An
  // engine-produced import/code-action whose specifier targets the IDE carrier
  // must strip back to the bare carrier too, alongside the `.verter.ts` API
  // carrier and the `.d.ts` alias.
  const ide = IDE_CARRIERS.find((row) => row.extension === c.extension);
  if (ide && !suffixes.includes(ide.ideSuffix)) suffixes.push(ide.ideSuffix);
  return suffixes.map((virtualSuffix) => ({
    pattern: new RegExp(escapeRegExp(`${c.extension}${virtualSuffix}`), "g"),
    carrierExt: c.extension,
    virtualSuffix,
    ambiguous: virtualSuffixIsAmbiguous(c.extension, virtualSuffix),
  }));
});

/** A path-like token: a run of non-whitespace, non-quote, non-paren chars. */
const PATH_TOKEN_REGEXP = /[^\s"'`()<>]+/g;

/**
 * Strip carrier virtual-file suffixes (`*.vue.ts` / `*.vue.d.ts` /
 * `*.vue.__verter_test.ts` / `*.svelte.ts` / …) embedded ANYWHERE in free-form
 * text (quick-fix descriptions, edit `newText`, display-part text) back to the
 * bare carrier path (`*.vue` / `*.svelte`). Carrier-generic: derived from the
 * manifest naming table, NOT a hardcoded `.vue` regex. Longest-suffix-first so
 * the `.d.ts` / testing variants are stripped before the bare `.ts` API suffix.
 *
 * AMBIGUOUS suffixes (a carrier's `{ext}.ts` that collides with a real self-file
 * module extension — Svelte's `.svelte.ts`) are only stripped when `fileExists`
 * is supplied AND the reconstructed backing carrier path resolves to a real
 * file, so a real `./store.svelte.ts` rune import in display text is NOT mangled
 * to `./store.svelte`. Without `fileExists`, ambiguous suffixes are left intact
 * (the no-host display path never corrupts a real rune path). Unambiguous
 * suffixes (Vue) always strip by shape.
 */
export function cleanupCarrierVirtualImportPath(
  text: string,
  fileExists?: (candidate: string) => boolean,
): string {
  let result = text;
  for (const {
    pattern,
    carrierExt,
    virtualSuffix,
    ambiguous,
  } of CARRIER_VIRTUAL_SUFFIX_STRIPPERS) {
    if (!ambiguous) {
      // Unambiguous (Vue `.vue.ts` / `.vue.d.ts` / `.vue.__verter_test.ts`):
      // a SHAPE match is always a virtual file — strip directly.
      result = result.replace(pattern, carrierExt);
      continue;
    }
    // Ambiguous (`.svelte.ts` etc.): strip a match ONLY when a backing-file
    // check proves the surrounding path token is the virtual API of a real
    // carrier (`X.{ext}` exists). Reconstruct the backing path from the path
    // token containing the match; with no host predicate, leave it intact.
    if (!fileExists) {
      continue;
    }
    result = result.replace(PATH_TOKEN_REGEXP, (token) =>
      stripAmbiguousSuffixInToken(token, carrierExt, virtualSuffix, fileExists),
    );
  }
  return result;
}

/**
 * Within a single path-like `token`, replace a trailing ambiguous virtual
 * suffix `{carrierExt}{virtualSuffix}` with the bare `{carrierExt}` ONLY when
 * the backing carrier path (`token` minus `virtualSuffix`) exists. A token that
 * is a real self-file module (`store.svelte.ts` with no `store.svelte`) is
 * returned unchanged. The check is anchored to the END of the token so a
 * mid-token coincidence never triggers.
 */
function stripAmbiguousSuffixInToken(
  token: string,
  carrierExt: string,
  virtualSuffix: string,
  fileExists: (candidate: string) => boolean,
): string {
  const full = `${carrierExt}${virtualSuffix}`;
  if (!token.endsWith(full)) {
    return token;
  }
  const backing = token.slice(0, token.length - virtualSuffix.length);
  return fileExists(backing) ? backing : token;
}

/**
 * Wrap a host existence predicate so a NON-ABSOLUTE backing candidate is
 * resolved against `path.dirname(containingFile)` before the underlying check.
 *
 * The backing-aware strippers ([`cleanupCarrierVirtualImportPath`],
 * [`stripVueVirtualSuffixBackingAware`]) reconstruct a backing carrier path
 * straight from the path token they are stripping — which is frequently
 * RELATIVE (a completion edit / display token `./Comp.svelte.ts` from a
 * containing file `/app/Parent.ts`). The underlying TS host `fileExists` only
 * recognises host-rooted / absolute paths, so a raw `fileExists("./Comp.svelte")`
 * answers `false` and the AMBIGUOUS Svelte virtual suffix is left un-stripped —
 * a Svelte-vs-Vue parity gap (Vue strips by shape, never needing the backing
 * check). Resolving the relative candidate against the containing file's
 * directory restores the backing proof, while the caller (the stripper) still
 * returns the ORIGINAL relative text minus the suffix — never an absolutized
 * path. An already-absolute candidate is passed through unchanged.
 *
 * This is a pure predicate wrapper; it never reads or normalises the text being
 * cleaned, so the unambiguous-Vue shape-strip path is untouched (it never calls
 * `fileExists` at all).
 */
export function containingFileAwareExists(
  fileExists: (candidate: string) => boolean,
  containingFile: string,
): (candidate: string) => boolean {
  const base = path.posix.dirname(normalizePath(containingFile));
  return (candidate: string): boolean => {
    const normalizedCandidate = normalizePath(candidate);
    // Resolve a RELATIVE backing candidate against the containing dir with the
    // POSIX resolver — `path.resolve` on Windows would emit backslashes + a
    // drive letter, which never matches the POSIX-normalised paths the rest of
    // this module (and the carrier store) operates on. An already-absolute
    // candidate (POSIX `/x` OR Windows `X:/x`) is passed through unchanged.
    const resolved = isNormalizedAbsolute(normalizedCandidate)
      ? normalizedCandidate
      : normalizePath(path.posix.resolve(base, normalizedCandidate));
    return fileExists(resolved);
  };
}

/** Whether a forward-slash-normalised path is absolute (POSIX `/x` or Windows `X:/x`). */
function isNormalizedAbsolute(p: string): boolean {
  return p.startsWith("/") || /^[A-Za-z]:\//.test(p);
}

export function isLikelyTestFileName(fileName: string): boolean {
  const normalized = normalizePath(fileName);
  return (
    /(?:^|\/)__tests__(?:\/|$)/.test(normalized) ||
    /(?:^|\/)__specs__(?:\/|$)/.test(normalized) ||
    /(?:^|\/)[^/]+\.(?:spec|test)\.[^/]+$/i.test(normalized)
  );
}

export function resolveVuePublicApiMode(
  exposeBindingsTesting: boolean,
  containingFile: string,
  isTestFile: (fileName: string) => boolean,
): VuePublicApiMode {
  if (!exposeBindingsTesting) {
    return "public";
  }

  return isTestFile(stripVueVirtualSuffix(containingFile)) ? "testing" : "public";
}

/** Whether `fileName` is a bare carrier file (`*.vue` / `*.svelte`). */
export const isVue = (fileName: string) => carrierFor(fileName) !== undefined;
export const isRelativeVue = (fileName: string) => isVue(fileName) && isRelative(fileName);

/** Whether `fileName` is a carrier API virtual file (`*.vue.ts` / `*.svelte.ts`). */
export const isVueTs = (fileName: string) => CARRIERS.some((c) => c.apiTs?.test(fileName) ?? false);
export const isRelativeVueTs = (fileName: string) => isVueTs(fileName) && isRelative(fileName);

/** Whether `fileName` is a carrier testing-API virtual file (`*.vue.__verter_test.ts`). */
export const isVueTestingTs = (fileName: string) =>
  CARRIERS.some((c) => c.testingTs?.test(fileName) ?? false);
