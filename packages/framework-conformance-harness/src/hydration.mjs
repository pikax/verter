// Hydration pairing #1 (ssr-hydration.md): official server / official
// client, run in a deterministic jsdom environment against the pinned
// official runtimes. This is the harness CONTROL pairing — it proves the
// mechanism end to end using only official-core artifacts.
//
// Pairings #2 ("Verter server / Verter client") and #3 ("official server /
// Verter client") need real candidate (Verter-compiled) output in this
// exact assembled shape. That does not exist yet at this point in the
// program (BV1/BS1, which build Verter's conformant Vue/Svelte backends,
// are downstream of BF2 in the DAG — see program-dag.toml: `B4 -> {BV1,
// BS1}` while BF2's only predecessor is BF1). This module exposes
// `hydrateVue`/`hydrateSvelteClient` as reusable, pluggable entry points so
// BV1/BS1 can drive pairings #2/#3 through the SAME mechanism once real
// candidate output exists — BF2 does not fabricate a placeholder candidate
// to claim those pairings today.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { JSDOM } from "jsdom";

import { HARNESS_ROOT } from "./paths.mjs";

const SCRATCH_DIR = path.join(HARNESS_ROOT, ".runtime-scratch");

function scratchModulePath(prefix, code) {
  mkdirSync(SCRATCH_DIR, { recursive: true });
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  const filePath = path.join(SCRATCH_DIR, `${prefix}-${digest}.mjs`);
  writeFileSync(filePath, code, "utf8");
  return filePath;
}

/**
 * Hydrates official-compiled Vue client output onto official-rendered SSR
 * HTML inside a fresh jsdom document.
 *
 * @param {string} ssrHtml the server-rendered markup (from execute-vue-runtime.mjs)
 * @param {string} clientCode module source compiled with backend "vdom"
 * @returns {Promise<{ ok: boolean, mismatched: boolean, finalHtml: string|null, error: string|null }>}
 */
export async function hydrateVue(ssrHtml, clientCode) {
  const dom = new JSDOM(`<!doctype html><html><body><div id="app">${ssrHtml}</div></body></html>`, {
    url: "http://localhost/",
  });
  const globalKeys = [
    "window",
    "document",
    "navigator",
    "Node",
    "Element",
    "HTMLElement",
    "SVGElement",
    "Text",
    "Comment",
    "DocumentFragment",
    "Event",
    "CustomEvent",
    "MouseEvent",
  ];
  const previous = new Map(globalKeys.map((key) => [key, globalThis[key]]));
  globalThis.window = dom.window;
  for (const key of globalKeys) {
    if (key !== "window" && dom.window[key] !== undefined) globalThis[key] = dom.window[key];
  }
  let mismatchLogged = false;
  const originalWarn = console.warn;
  console.warn = (...args) => {
    if (args.some((a) => String(a).includes("Hydration"))) mismatchLogged = true;
    originalWarn(...args);
  };
  try {
    const filePath = scratchModulePath("vue-client", clientCode);
    const [{ createSSRApp }, mod] = await Promise.all([
      import("vue"),
      import(new URL(`file://${filePath}`).href),
    ]);
    const component = mod.default;
    const container = dom.window.document.getElementById("app");
    const app = createSSRApp(component);
    app.mount(container);
    return {
      ok: true,
      mismatched: mismatchLogged,
      finalHtml: container.innerHTML,
      error: null,
    };
  } catch (error) {
    return {
      ok: false,
      mismatched: mismatchLogged,
      finalHtml: null,
      error: String(error?.stack ?? error),
    };
  } finally {
    console.warn = originalWarn;
    for (const [key, value] of previous) {
      if (value === undefined) delete globalThis[key];
      else globalThis[key] = value;
    }
  }
}

/**
 * Hydrates official-compiled Svelte client output onto official-rendered
 * SSR HTML. Svelte's top-level `mount`/`hydrate` API lives only on the
 * "browser" export condition of the `svelte` package (see
 * packages/svelte-runtime-tests/vitest.config.ts's own note on this in the
 * wider repo) — this harness runs that half in a child process launched
 * with `--conditions=browser` rather than pulling that condition into the
 * whole workspace's module resolution.
 *
 * @returns {{ ok: boolean, mismatched: boolean, finalHtml: string|null, error: string|null }}
 */
export function hydrateSvelteClient(ssrHtml, clientCode, propsJson = "{}") {
  const clientPath = scratchModulePath("svelte-client", clientCode);
  const runnerSource = `
import { JSDOM } from ${JSON.stringify(path.join(HARNESS_ROOT, "node_modules", "jsdom", "lib", "api.js"))};
import { mount, hydrate } from "svelte";
const dom = new JSDOM(${JSON.stringify(
    '<!doctype html><html><body><div id="app"></div></body></html>',
  )}, { url: "http://localhost/" });
globalThis.window = dom.window;
for (const key of ["document", "navigator", "Node", "Element", "HTMLElement", "SVGElement", "Text", "Comment", "DocumentFragment", "Event", "CustomEvent", "MouseEvent"]) {
  if (dom.window[key] !== undefined) globalThis[key] = dom.window[key];
}
const container = dom.window.document.getElementById("app");
container.innerHTML = ${JSON.stringify(ssrHtml)};
const mod = await import(${JSON.stringify(`file://${clientPath}`)});
try {
  hydrate(mod.default, { target: container, props: ${propsJson} });
  process.stdout.write(JSON.stringify({ ok: true, finalHtml: container.innerHTML }));
} catch (error) {
  process.stdout.write(JSON.stringify({ ok: false, error: String(error?.stack ?? error) }));
}
`;
  const runnerPath = scratchModulePath("svelte-hydrate-runner", runnerSource);
  const result = spawnSync(process.execPath, ["--conditions=browser", runnerPath], {
    cwd: HARNESS_ROOT,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    return { ok: false, mismatched: false, finalHtml: null, error: result.stderr || result.stdout };
  }
  try {
    const parsed = JSON.parse(result.stdout.trim());
    return {
      ok: parsed.ok,
      mismatched: false,
      finalHtml: parsed.finalHtml ?? null,
      error: parsed.error ?? null,
    };
  } catch (error) {
    return {
      ok: false,
      mismatched: false,
      finalHtml: null,
      error: `unparseable runner output: ${result.stdout}`,
    };
  }
}

export function cleanupHydrationScratch() {
  rmSync(SCRATCH_DIR, { recursive: true, force: true });
}
