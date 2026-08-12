// Hermetic invocation of the pinned official Svelte 5.56.8 compiler
// (`svelte/compiler`). Never writes an expectation; only produces a raw
// compilation artifact for the caller (golden generator or comparator).
//
// Package pin is asserted on import — any drift from domain-pin.mjs throws
// before a single line of the oracle compiler runs.

import { compile } from "svelte/compiler";

import { SVELTE_DOMAIN, EVIDENCE_LOCK_DIGESTS } from "./domain-pin.mjs";
import { assertPackagesPinned } from "./package-pin.mjs";
import { HARNESS_ROOT, SVELTE_EVIDENCE_LOCK } from "./paths.mjs";

let pinned = false;
export function assertSveltePinned() {
  if (pinned) return;
  assertPackagesPinned(
    SVELTE_DOMAIN,
    HARNESS_ROOT,
    SVELTE_EVIDENCE_LOCK,
    EVIDENCE_LOCK_DIGESTS.sveltePackageLockSha256,
  );
  pinned = true;
}

/**
 * @typedef {"client"|"server"} SvelteGenerateTarget
 */

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
      start: error?.start ?? null,
    });
    return { code: null, map: null, css: null, diagnostics, generate };
  }

  for (const warning of result.warnings ?? []) {
    diagnostics.push({
      kind: "warning",
      code: warning.code ?? null,
      message: warning.message,
      start: warning.start ?? null,
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
