// Deterministic execution of compiled Vue output against the pinned OFFICIAL
// runtime (`vue`'s `createSSRApp` + `@vue/server-renderer`'s
// `renderToString`) — never a Verter-owned runtime, never a simplified
// substitute (ssr-hydration.md).
//
// The compiled module is written to a scratch file INSIDE this package's
// own tree so its bare `from "vue"` imports resolve through ordinary Node
// module resolution against the exact pinned `vue`/`@vue/server-renderer`
// devDependencies — the same real-package resolution `checkLinkValidity`
// proves statically, exercised here dynamically at runtime.

import { createHash } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { HARNESS_ROOT } from "./paths.mjs";

const SCRATCH_DIR = path.join(HARNESS_ROOT, ".runtime-scratch");

function scratchModulePath(code) {
  mkdirSync(SCRATCH_DIR, { recursive: true });
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  const filePath = path.join(SCRATCH_DIR, `vue-ssr-${digest}.mjs`);
  writeFileSync(filePath, code, "utf8");
  return filePath;
}

/**
 * @param {string} ssrCode module source compiled with backend "ssr"
 * @returns {Promise<{ ok: boolean, html: string|null, error: string|null }>}
 */
export async function executeVueSsr(ssrCode) {
  const filePath = scratchModulePath(ssrCode);
  try {
    const [{ createSSRApp }, { renderToString }, mod] = await Promise.all([
      import("vue"),
      import("@vue/server-renderer"),
      import(pathToFileURL(filePath).href),
    ]);
    const component = mod.default;
    const app = createSSRApp(component);
    const html = await renderToString(app);
    return { ok: true, html, error: null };
  } catch (error) {
    return { ok: false, html: null, error: String(error?.stack ?? error) };
  }
}

export function cleanupScratch() {
  rmSync(SCRATCH_DIR, { recursive: true, force: true });
}
