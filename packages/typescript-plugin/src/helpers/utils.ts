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

const isRelative = (fileName: string) => RELATIVE_REGEXP.test(fileName);

export function normalizePath(fileName: string): string {
  return fileName.replace(/\\/g, "/");
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
 * The carrier virtual-suffix strip rules, derived ONCE from the carrier naming
 * table, longest-suffix-first so `*.vue.__verter_test.ts` and `*.vue.d.ts` win
 * over `*.vue.ts` (and every carrier — `.vue`, `.svelte` — is covered). Each
 * rule maps a carrier virtual-file suffix (e.g. `.vue.ts`) embedded anywhere in
 * a string back to the bare carrier extension (`.vue`).
 */
const CARRIER_VIRTUAL_SUFFIX_STRIPPERS: readonly { pattern: RegExp; carrierExt: string }[] = CARRIERS.flatMap(
  (c) => {
    const suffixes: string[] = [];
    if (c.testingApiSuffix) suffixes.push(`${c.extension}${c.testingApiSuffix}`);
    // The `.d.ts` accepted-spelling alias is `{carrier_ext}.d.ts` uniformly.
    suffixes.push(`${c.extension}.d.ts`);
    if (c.apiSuffix) suffixes.push(`${c.extension}${c.apiSuffix}`);
    return suffixes.map((suffix) => ({
      pattern: new RegExp(escapeRegExp(suffix), "g"),
      carrierExt: c.extension,
    }));
  },
);

/**
 * Strip carrier virtual-file suffixes (`*.vue.ts` / `*.vue.d.ts` /
 * `*.vue.__verter_test.ts` / `*.svelte.ts` / …) embedded ANYWHERE in free-form
 * text (quick-fix descriptions, edit `newText`, display-part text) back to the
 * bare carrier path (`*.vue` / `*.svelte`). Carrier-generic: derived from the
 * manifest naming table, NOT a hardcoded `.vue` regex. Longest-suffix-first so
 * the `.d.ts` / testing variants are stripped before the bare `.ts` API suffix.
 */
export function cleanupCarrierVirtualImportPath(text: string): string {
  let result = text;
  for (const { pattern, carrierExt } of CARRIER_VIRTUAL_SUFFIX_STRIPPERS) {
    result = result.replace(pattern, carrierExt);
  }
  return result;
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
