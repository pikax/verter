// Deterministic execution of compiled Vue output against the pinned OFFICIAL
// runtime (`vue`'s `createSSRApp` + `@vue/server-renderer`'s
// `renderToString`) — never a Verter-owned runtime, never a simplified
// substitute (ssr-hydration.md).
//
// The runtime is loaded from the ISOLATED per-domain installation realized
// from the committed oracle lock (oracle-install.mjs), and the compiled
// module is written to a scratch file INSIDE that install tree so its bare
// `from "vue"` imports resolve through ordinary Node module resolution
// against the exact realized closure — and against the SAME module
// instances this executor uses (one `vue` instance graph, which SSR
// rendering requires).

import { createHash, randomUUID } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { importOracleModule, oracleScratchDir } from "./oracle-install.mjs";

// Instance-scoped scratch: parallel test workers each load their own module
// instance, so cleanup can never delete another worker's in-flight modules.
const SCRATCH_LABEL = `vue-ssr-${randomUUID()}`;
let scratchDir = null;

function scratchModulePath(code) {
  if (scratchDir === null) scratchDir = oracleScratchDir("vue", SCRATCH_LABEL);
  mkdirSync(scratchDir, { recursive: true }); // cleanup may have removed it mid-run
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  const filePath = path.join(scratchDir, `vue-ssr-${digest}.mjs`);
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
      importOracleModule("vue", "vue"),
      importOracleModule("vue", "@vue/server-renderer"),
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
  if (scratchDir !== null) rmSync(scratchDir, { recursive: true, force: true });
}
