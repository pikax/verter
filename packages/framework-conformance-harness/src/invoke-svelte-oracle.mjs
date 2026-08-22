// Hermetic invocation of the pinned official Svelte 5.56.10 compiler
// (`svelte/compiler`). Never writes an expectation; only produces a raw
// compilation artifact for the caller (golden generator or comparator).
//
// The compiler is loaded DYNAMICALLY from the isolated per-domain
// installation realized from the committed oracle lock (oracle-install.mjs)
// — never from workspace dependency resolution, and never via a static
// top-level import of the oracle package. The ESM entry is used
// deliberately: Svelte's ESM compiler (`src/compiler/index.js`) imports its
// REAL dependencies — `acorn`, `@sveltejs/acorn-typescript`, … — from the
// surrounding node_modules tree, so loading it from the isolated install is
// what makes the actual parser/plugin combination the locked one (the CJS
// bundle inlines those dependencies and would mask the closure entirely).
// The full validation gate runs at this module's load, before a single line
// of the oracle compiler is evaluated.

import { SVELTE_DOMAIN } from "./domain-pin.mjs";
import { PackageDriftError } from "./package-pin.mjs";
import { ensureOracleDomain, importOracleModule } from "./oracle-install.mjs";

/** Runs the isolated-install validation gate without loading the compiler. */
export function assertSveltePinned() {
  ensureOracleDomain("svelte");
}

// Gate first, then load the oracle from the validated isolated install.
// (Top-level await: importers of this module observe either a fully-gated,
// fully-loaded compiler or the gate's refusal error — never a half state.)
assertSveltePinned();
const { compile, VERSION } = await importOracleModule("svelte", "svelte/compiler");

// Loaded-module identity gate: the compiler ACTUALLY IN USE attests its own
// version against the domain pin. The install-tree layers above prove the
// isolated closure; this proves the loaded module IS from that closure —
// a load that slipped through any other resolution path (e.g. a
// workspace-hoisted svelte at a different version) refuses here.
if (VERSION !== SVELTE_DOMAIN.packageVersion) {
  throw new PackageDriftError(
    `loaded svelte/compiler reports version ${VERSION}, pinned ${SVELTE_DOMAIN.packageVersion}`,
    { expected: SVELTE_DOMAIN.packageVersion, actual: VERSION, layer: "loaded-module-identity" },
  );
}

/** Version the LOADED oracle compiler attests — for identity self-tests. */
export function svelteOracleCompilerVersion() {
  return VERSION;
}

/**
 * @typedef {"client"|"server"} SvelteGenerateTarget
 */

/** Captures the official compiler's span exactly: line, column (start AND end). */
function toPosition(position) {
  if (position === null || position === undefined) return null;
  return { line: position.line ?? null, column: position.column ?? null };
}

/**
 * Compiles one independently-authored Svelte component fixture with the
 * official pinned compiler.
 *
 * @param {string} source raw `.svelte` component text
 * @param {string} filename stable label, never read from disk
 * @param {{ generate: SvelteGenerateTarget, runes: boolean, dev: boolean, sourceMap: boolean }} options
 * @returns {{ code: string|null, map: object|null, css: object|null,
 *   diagnostics: Array<object>, generate: SvelteGenerateTarget }}
 */
export function compileSvelteFixture(source, filename, options) {
  assertSveltePinned();
  const { generate, runes, dev, sourceMap } = options;
  const diagnostics = [];

  // NOTE: Svelte's `sourcemap` compile option accepts an INPUT map to chain
  // from (e.g. a preprocessor's), not an output on/off boolean — the
  // compiler always produces `js.map`. Passing a boolean there crashes the
  // internal map builder. The sourceMap axis therefore only gates whether
  // this function RETURNS the always-produced map, never a compile input.
  let result;
  try {
    result = compile(source, {
      filename,
      generate,
      runes,
      dev,
      css: "injected",
    });
  } catch (error) {
    diagnostics.push({
      kind: "compile-error",
      code: error?.code ?? null,
      message: String(error?.message ?? error),
      source: filename,
      start: toPosition(error?.start),
      end: toPosition(error?.end),
    });
    return { code: null, map: null, css: null, diagnostics, generate };
  }

  for (const warning of result.warnings ?? []) {
    diagnostics.push({
      kind: "warning",
      code: warning.code ?? null,
      message: warning.message,
      source: filename,
      start: toPosition(warning.start),
      end: toPosition(warning.end),
    });
  }

  return {
    code: result.js.code,
    map: sourceMap ? (result.js.map ?? null) : null,
    css: result.css ?? null,
    diagnostics,
    generate,
  };
}
