// Deterministic execution of compiled Svelte output against the pinned
// OFFICIAL server runtime (`svelte/server`'s `render`) — never a Verter-owned
// runtime.

import { createHash } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { HARNESS_ROOT } from "./paths.mjs";

const SCRATCH_DIR = path.join(HARNESS_ROOT, ".runtime-scratch");

function scratchModulePath(code) {
  mkdirSync(SCRATCH_DIR, { recursive: true });
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  const filePath = path.join(SCRATCH_DIR, `svelte-ssr-${digest}.mjs`);
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
      import("svelte/server"),
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
  rmSync(SCRATCH_DIR, { recursive: true, force: true });
}
