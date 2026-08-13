// Behavioral execution of a compiled CLIENT (vapor-backend) artifact
// through the pinned OFFICIAL with-vapor runtime — never a Verter-owned
// runtime, never a simplified substitute. This is the check that observes
// the `__vapor` interop marker: `vaporInteropPlugin` routes a child to the
// vapor mount path only when the component object carries `__vapor: true`
// (or a `defineVaporComponent` wrapper), so a marked artifact renders the
// fixture's real DOM warning-free and an unmarked one mis-renders through
// the VDOM path with runtime warnings.
//
// The vapor runtime is only published in Vue's ESM browser/bundler builds —
// the CJS artifact the install's `vue` specifier resolves to for Node
// carries no vapor exports — so both LINK checking and EXECUTION of a
// vapor artifact must resolve `vue` against the with-vapor runtime entry
// (`vaporRuntimeHref`) instead of the Node default. Execution redirects the
// module's `from "vue"` imports to that entry by SYNTAX LOCATION (the
// ImportDeclaration source literal's exact span), so the module and the
// mounting host share ONE runtime instance graph.

import { randomUUID } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { parseModule } from "./normalize.mjs";
import { ensureOracleDomain, oracleScratchDir } from "./oracle-install.mjs";

/** The pinned install's self-contained vapor-capable runtime build. */
export const VAPOR_RUNTIME_RELATIVE = "node_modules/vue/dist/vue.runtime-with-vapor.esm-browser.js";

/** file: URL of the vapor-capable runtime inside the validated install. */
export function vaporRuntimeHref() {
  const { installDir } = ensureOracleDomain("vue");
  return pathToFileURL(path.join(installDir, VAPOR_RUNTIME_RELATIVE)).href;
}

/**
 * Redirects every `from "vue"` import of an assembled module to
 * `runtimeHref`, by syntax location — a string literal that merely contains
 * the text is an ordinary expression node, never a rewritten source.
 */
function redirectVueImports(code, runtimeHref) {
  const ast = parseModule(code, "vapor-interop-module");
  const sources = ast.body
    .filter((s) => s.type === "ImportDeclaration" && s.source.value === "vue")
    .map((s) => s.source)
    .sort((a, b) => b.start - a.start);
  let out = code;
  for (const source of sources) {
    out = out.slice(0, source.start) + JSON.stringify(runtimeHref) + out.slice(source.end);
  }
  return out;
}

// Instance-scoped scratch: parallel test workers each load their own module
// instance, so cleanup can never delete another worker's in-flight modules.
const SCRATCH_LABEL = `vue-vapor-interop-${randomUUID()}`;
let scratchDir = null;

// ONE document per process: the pinned runtime build captures `document` at
// module-evaluation time (module scope: `const doc = typeof document !==
// "undefined" ? document : null`, plus a `templateContainer` created FROM
// that capture), so a per-mount DOM would hand the (cached) runtime nodes
// from a foreign document on every mount after the first — and an import
// that happens before ANY document exists pins the capture to `null` for
// the life of the process's ESM cache. Both hazards resolve the same way:
// the shared document below must be installed before the runtime module's
// FIRST evaluation, and every later consumer reuses it.
let dom = null;

const DOM_GLOBAL_KEYS = [
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

/** The shared process-wide jsdom instance (created on first demand). */
async function ensureDom() {
  // jsdom is loaded lazily so the CLI path only pays for it when a vapor
  // runtime axis actually runs.
  const { JSDOM } = await import("jsdom");
  if (dom === null) {
    dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost/" });
  }
  return dom;
}

/**
 * Installs the shared jsdom's globals; returns a restore thunk that puts
 * every previous global back exactly (deleting the ones that were absent).
 */
function installDomGlobals(sharedDom) {
  const previous = new Map(DOM_GLOBAL_KEYS.map((key) => [key, globalThis[key]]));
  globalThis.window = sharedDom.window;
  for (const key of DOM_GLOBAL_KEYS) {
    if (key !== "window" && sharedDom.window[key] !== undefined)
      globalThis[key] = sharedDom.window[key];
  }
  return () => {
    for (const [key, value] of previous) {
      if (value === undefined) delete globalThis[key];
      else globalThis[key] = value;
    }
  };
}

/**
 * Idempotently evaluates the pinned with-vapor runtime with the shared
 * process document installed. The runtime captures `document` ONCE at
 * module-evaluation time, so whichever consumer triggers the first
 * `import()` of `vaporRuntimeHref()` decides that capture for the whole
 * process — a consumer that imports it merely to inspect exports (the link
 * axis checking a vapor artifact's named imports) would otherwise pin the
 * capture to `null` and break every later mount that reaches the runtime's
 * VDOM-fragment path. Any composition that can import the runtime before a
 * mount runs MUST call this first; it shares the same process-wide document
 * every mount uses, so the capture stays correct for the life of the process.
 */
export async function ensureVaporRuntimePreloaded() {
  const runtimeHref = vaporRuntimeHref();
  const restore = installDomGlobals(await ensureDom());
  // The dev runtime build prints an informational banner on first import
  // (console.info/log); suppress it so a CLI consumer's stdout stays
  // exactly the machine-readable report it prints itself.
  const originalInfo = console.info;
  const originalLog = console.log;
  console.info = () => {};
  console.log = () => {};
  try {
    await import(runtimeHref);
  } finally {
    console.info = originalInfo;
    console.log = originalLog;
    restore();
  }
}

/**
 * Mounts a compiled client module through the pinned with-vapor runtime
 * under `createApp` + `vaporInteropPlugin` in jsdom.
 *
 * @param {string} moduleCode assembled module source (plain JS)
 * @returns {Promise<{ ok: boolean, component: object|null, html: string|null,
 *   warnings: string[], error: string|null }>}
 */
export async function executeVueVaporInterop(moduleCode) {
  const runtimeHref = vaporRuntimeHref();
  if (scratchDir === null) scratchDir = oracleScratchDir("vue", SCRATCH_LABEL);
  mkdirSync(scratchDir, { recursive: true }); // cleanup may have removed it mid-run
  const modulePath = path.join(scratchDir, `interop-${randomUUID().slice(0, 8)}.mjs`);

  let component = null;
  const warnings = [];
  let html = null;
  let error = null;
  try {
    writeFileSync(modulePath, redirectVueImports(moduleCode, runtimeHref), "utf8");
    const sharedDom = await ensureDom();
    const restoreGlobals = installDomGlobals(sharedDom);
    const originalWarn = console.warn;
    console.warn = (...args) => {
      warnings.push(args.map(String).join(" "));
    };
    // The dev runtime build prints an informational banner on first import
    // (console.info/log). Suppress it so a CLI consumer's stdout stays
    // exactly the machine-readable report it prints itself.
    const originalInfo = console.info;
    const originalLog = console.log;
    console.info = () => {};
    console.log = () => {};
    const container = sharedDom.window.document.createElement("div");
    sharedDom.window.document.body.appendChild(container);
    try {
      const runtime = await import(runtimeHref);
      const mod = await import(pathToFileURL(modulePath).href);
      component = mod.default;
      try {
        const app = runtime.createApp({ render: () => runtime.h(component) });
        app.use(runtime.vaporInteropPlugin);
        app.config.warnHandler = (message) => warnings.push(message);
        app.mount(container);
      } catch (mountError) {
        error = String(mountError?.stack ?? mountError);
      }
      html = container.innerHTML;
    } finally {
      container.remove();
      console.warn = originalWarn;
      console.info = originalInfo;
      console.log = originalLog;
      restoreGlobals();
    }
  } catch (loadError) {
    error = String(loadError?.stack ?? loadError);
  }
  return { ok: error === null, component, html, warnings, error };
}

export function cleanupScratch() {
  if (scratchDir !== null) rmSync(scratchDir, { recursive: true, force: true });
}
