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
import { createHash, randomUUID } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { JSDOM } from "jsdom";

import { HARNESS_ROOT } from "./paths.mjs";
import { importOracleModule, oracleScratchDir } from "./oracle-install.mjs";

// Instance-scoped scratch INSIDE each isolated oracle install tree
// (oracle-install.mjs): compiled client modules' bare `vue` / `svelte/*`
// imports resolve against the exact realized closure and share one module
// instance graph with the runtime entry points used here. Parallel test
// workers each load their own module instance, so cleanup can never delete
// another worker's in-flight modules.
const SCRATCH_LABEL = `hydrate-${randomUUID()}`;
const scratchDirs = new Map();

function scratchModulePath(framework, prefix, code) {
  let dir = scratchDirs.get(framework);
  if (dir === undefined) {
    dir = oracleScratchDir(framework, SCRATCH_LABEL);
    scratchDirs.set(framework, dir);
  }
  mkdirSync(dir, { recursive: true }); // cleanup may have removed it mid-run
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  const filePath = path.join(dir, `${prefix}-${digest}.mjs`);
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
    const filePath = scratchModulePath("vue", "vue-client", clientCode);
    const [{ createSSRApp }, mod] = await Promise.all([
      importOracleModule("vue", "vue"),
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
 * MISMATCH DETECTION. Svelte has no single always-on equivalent of Vue's
 * hydration warning, and no one signal covers every mismatch class its
 * runtime can produce (read from the pinned 5.56.8 client runtime,
 * internal/client/render.js + dom/hydration.js), so the runner combines
 * the three REAL signals that together do:
 *
 *  1. the runtime's OWN `hydration_mismatch` console warning — emitted in
 *     both dev and prod builds (prod logs the
 *     https://svelte.dev/e/hydration_mismatch URL) when the hydration walk
 *     finds torn structure mid-claim; the direct analogue of Vue's
 *     intercepted warn;
 *  2. server-node reuse identity — correct hydration CLAIMS the
 *     server-rendered nodes; Svelte's recovery path (e.g. marker-less
 *     server HTML: `if (!anchor) throw HYDRATION_ERROR`) silently clears
 *     the target and re-mounts fresh WITHOUT any warning, and afterwards
 *     the final DOM equals a fresh render exactly — so that class is
 *     detectable only by observing that none of the pre-hydration server
 *     child nodes remain connected. Tracked over ALL initial child nodes —
 *     text nodes and the hydration marker/anchor comments included, not
 *     only elements — because a component whose root is a bare text
 *     binding has ZERO element children, and an element-only reuse check
 *     would be vacuously true for exactly the recovery class it exists to
 *     catch (wrong markerless server text, torn markers). Only a genuinely
 *     EMPTY server root — nothing to reuse — is legitimately vacuous;
 *  3. fresh-render comparison (the divergence oracle): a second, detached
 *     `mount` of the same component with the same props, compared against
 *     the hydrated container on comment-free, attribute-order-normalized
 *     serializations — catches silent same-shape adoption, where prod
 *     hydration claims wrong server nodes without checking tag names or
 *     static content and the wrong DOM survives hydration.
 *
 * @returns {{ ok: boolean, mismatched: boolean, finalHtml: string|null, error: string|null }}
 */
export function hydrateSvelteClient(ssrHtml, clientCode, propsJson = "{}") {
  const clientPath = scratchModulePath("svelte", "svelte-client", clientCode);
  const runnerSource = `
import { JSDOM } from ${JSON.stringify(path.join(HARNESS_ROOT, "node_modules", "jsdom", "lib", "api.js"))};
import { mount, hydrate, flushSync } from "svelte";
const dom = new JSDOM(${JSON.stringify(
    '<!doctype html><html><body><div id="app"></div></body></html>',
  )}, { url: "http://localhost/" });
globalThis.window = dom.window;
for (const key of ["document", "navigator", "Node", "Element", "HTMLElement", "SVGElement", "Text", "Comment", "DocumentFragment", "Event", "CustomEvent", "MouseEvent"]) {
  if (dom.window[key] !== undefined) globalThis[key] = dom.window[key];
}
const container = dom.window.document.getElementById("app");
container.innerHTML = ${JSON.stringify(ssrHtml)};
const props = ${propsJson};

// Signal 1: Svelte's own hydration warning (dev AND prod spellings both
// contain "hydrat"). Installed before the client module can run.
let hydrationWarned = false;
const originalWarn = console.warn;
console.warn = (...args) => {
  if (args.some((a) => /hydrat/i.test(String(a)))) hydrationWarned = true;
  originalWarn(...args);
};

// Signal 2 precondition: EVERY server-rendered child node as parsed —
// text and marker-comment nodes included (see the function doc; a
// text-only root has zero element children, so filtering to elements
// would make the reuse signal vacuously clean for its recovery class).
const initialServerNodes = [...container.childNodes];

const mod = await import(${JSON.stringify(`file://${clientPath}`)});
try {
  hydrate(mod.default, { target: container, props });
  flushSync();
  const serverNodesReused =
    initialServerNodes.length === 0 || initialServerNodes.some((n) => container.contains(n));

  // Signal 3: detached fresh render of the same component/props.
  const freshTarget = dom.window.document.createElement("div");
  mount(mod.default, { target: freshTarget, props });
  flushSync();
  const serialize = (node) => {
    if (node.nodeType === 3) return node.data === "" ? "" : JSON.stringify(node.data);
    // Comments (hydration markers/anchors) are erased from THIS comparison
    // by design: a fresh client mount never produces the SSR boundary
    // markers a correctly-hydrated container retains, so a comment-aware
    // fresh-render comparison would flag every CORRECT hydration. Marker
    // survival is owned by the reuse-identity signal above, which tracks
    // the initial marker comments as nodes.
    if (node.nodeType !== 1) return "";
    const attrs = [...node.attributes].map((a) => a.name + "=" + JSON.stringify(a.value)).sort().join(" ");
    const children = [...node.childNodes].map(serialize).join("");
    const tag = node.tagName.toLowerCase();
    return "<" + tag + (attrs ? " " + attrs : "") + ">" + children + "</" + tag + ">";
  };
  const serializeChildren = (parent) => [...parent.childNodes].map(serialize).join("");
  const matchesFreshRender = serializeChildren(container) === serializeChildren(freshTarget);

  process.stdout.write(JSON.stringify({
    ok: true,
    mismatched: hydrationWarned || !serverNodesReused || !matchesFreshRender,
    finalHtml: container.innerHTML,
  }));
} catch (error) {
  process.stdout.write(JSON.stringify({ ok: false, mismatched: hydrationWarned, error: String(error?.stack ?? error) }));
}
`;
  // The runner lives INSIDE the isolated svelte install tree, so its
  // `import { mount, hydrate } from "svelte"` resolves the realized closure
  // (browser condition included) — never the workspace store.
  const runnerPath = scratchModulePath("svelte", "svelte-hydrate-runner", runnerSource);
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
      mismatched: parsed.mismatched === true,
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
  for (const dir of scratchDirs.values()) rmSync(dir, { recursive: true, force: true });
  scratchDirs.clear();
}
