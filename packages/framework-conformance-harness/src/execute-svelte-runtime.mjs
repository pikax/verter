// Deterministic execution of compiled Svelte output against the pinned
// OFFICIAL server runtime (`svelte/server`'s `render`) — never a
// Verter-owned runtime. Runtime and compiled scratch module both load from
// the isolated per-domain installation (oracle-install.mjs), so bare
// `svelte/*` imports resolve against the exact realized closure and share
// one module instance graph.

import { createHash, randomUUID } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { importOracleModule, oracleScratchDir } from "./oracle-install.mjs";

// Instance-scoped scratch: parallel test workers each load their own module
// instance, so cleanup can never delete another worker's in-flight modules.
const SCRATCH_LABEL = `svelte-ssr-${randomUUID()}`;
let scratchDir = null;
let scratchModuleSequence = 0;

function scratchModulePath(code) {
  if (scratchDir === null) scratchDir = oracleScratchDir("svelte", SCRATCH_LABEL);
  mkdirSync(scratchDir, { recursive: true }); // cleanup may have removed it mid-run
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  // Batch mode may execute byte-identical server modules more than once.
  // Node caches ESM by URL, so a per-execution suffix is required to prevent
  // module-level component state from leaking into a later candidate case.
  const filePath = path.join(scratchDir, `svelte-ssr-${digest}-${scratchModuleSequence++}.mjs`);
  writeFileSync(filePath, code, "utf8");
  return filePath;
}

/**
 * @param {string} serverCode module source compiled with generate:"server"
 * @param {object} props
 * @returns {Promise<{ ok: boolean, html: string|null, error: string|null }>}
 */
export async function executeSvelteSsr(serverCode, props = {}) {
  const filePath = scratchModulePath(serverCode);
  try {
    const [{ render }, mod] = await Promise.all([
      importOracleModule("svelte", "svelte/server"),
      import(pathToFileURL(filePath).href),
    ]);
    const component = mod.default;
    const result = render(component, { props });
    return { ok: true, html: result.body, error: null };
  } catch (error) {
    return { ok: false, html: null, error: String(error?.stack ?? error) };
  }
}

export function cleanupScratch() {
  if (scratchDir !== null) rmSync(scratchDir, { recursive: true, force: true });
}
