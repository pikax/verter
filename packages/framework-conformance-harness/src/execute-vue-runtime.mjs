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

import { parseModule } from "./normalize.mjs";
import { ensureOracleDomain, importOracleModule, oracleScratchDir } from "./oracle-install.mjs";

// Instance-scoped scratch: parallel test workers each load their own module
// instance, so cleanup can never delete another worker's in-flight modules.
const SCRATCH_LABEL = `vue-ssr-${randomUUID()}`;
let scratchDir = null;
let scratchModuleSequence = 0;

function scratchModulePath(code) {
  if (scratchDir === null) scratchDir = oracleScratchDir("vue", SCRATCH_LABEL);
  mkdirSync(scratchDir, { recursive: true }); // cleanup may have removed it mid-run
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  // A batch may execute the same source more than once. A unique URL gives
  // each case a fresh candidate module instance while the pinned framework
  // runtime remains shared by the process.
  const filePath = path.join(scratchDir, `vue-ssr-${digest}-${scratchModuleSequence++}.mjs`);
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

// ── Client mount (CSS custom properties, reactive updates) ────────────
//
// CSS `v-bind()` lowers to a `useCssVars()` registration whose applied
// value is only observable on a MOUNTED element: the runtime prepends `--`
// to each registered key and writes it with `style.setProperty` from a
// post-flush effect. Neither SSR rendering nor reading the module text can
// see that, so this mount exists to observe the real custom properties the
// pinned official runtime actually sets, initially and after a reactive
// change.
//
// The module is mounted against the PLAIN browser runtime entry
// (`vue.runtime.esm-browser.js`) rather than the with-vapor entry the
// interop executor uses. A runtime module captures `document` once at
// evaluation time, so two consumers sharing one entry would have to share
// one document; two DISTINCT entries are two module instances, and each
// owns its own capture. Same pinned install, different entry.

/** The pinned install's plain (non-vapor) browser runtime build. */
export const CLIENT_RUNTIME_RELATIVE = "node_modules/vue/dist/vue.runtime.esm-browser.js";

/** file: URL of the plain browser runtime inside the validated install. */
export function clientRuntimeHref() {
  const { installDir } = ensureOracleDomain("vue");
  return pathToFileURL(path.join(installDir, CLIENT_RUNTIME_RELATIVE)).href;
}

/**
 * Adapts an assembled SFC client module for a standalone mount, by syntax
 * location — a string literal that merely contains the text is an ordinary
 * expression node, never a rewritten source.
 *
 * 1. Every `from "vue"` import is redirected to `runtimeHref`: the module and
 *    the mounting host must share ONE runtime instance graph.
 * 2. Side-effect imports of the Vue virtual-module namespace
 *    (`"<id>?vue&type=style|custom&..."`) are REMOVED. Those are the dev
 *    server's style/custom-block delivery channel — only the bundler
 *    resolves them; standalone Node cannot, and the stylesheet bytes are
 *    irrelevant to what a mount observes (jsdom applies no stylesheet, and
 *    custom properties come from the `useCssVars` registration, not the
 *    sheet).
 */
function redirectVueImports(code, runtimeHref) {
  const ast = parseModule(code, "vue-client-mount-module");
  const edits = [];
  for (const statement of ast.body) {
    if (statement.type !== "ImportDeclaration") continue;
    if (statement.source.value === "vue") {
      edits.push({
        start: statement.source.start,
        end: statement.source.end,
        text: JSON.stringify(runtimeHref),
      });
    } else if (
      statement.specifiers.length === 0 &&
      String(statement.source.value).includes("?vue&type=")
    ) {
      edits.push({ start: statement.start, end: statement.end, text: "" });
    }
  }
  edits.sort((a, b) => b.start - a.start);
  let out = code;
  for (const edit of edits) {
    out = out.slice(0, edit.start) + edit.text + out.slice(edit.end);
  }
  return out;
}

const CLIENT_DOM_GLOBAL_KEYS = [
  "window",
  "document",
  // `useCssVars` observes the mounted subtree, so the mount fails outright
  // without it.
  "MutationObserver",
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

// ONE document per process for this entry, installed before the runtime's
// first evaluation (see the capture note above).
let clientDom = null;

async function ensureClientDom() {
  const { JSDOM } = await import("jsdom");
  if (clientDom === null) {
    clientDom = new JSDOM("<!doctype html><html><body></body></html>", {
      url: "http://localhost/",
    });
  }
  return clientDom;
}

/**
 * Installs the shared jsdom's globals; returns a restore thunk that puts
 * every previous descriptor back exactly (deleting the ones that were absent).
 */
function installClientDomGlobals(sharedDom) {
  const previous = new Map();
  const installed = [];

  const restore = () => {
    const errors = [];
    for (const key of installed.toReversed()) {
      try {
        const descriptor = previous.get(key);
        if (descriptor === undefined) delete globalThis[key];
        else Object.defineProperty(globalThis, key, descriptor);
      } catch (error) {
        errors.push(error);
      }
    }
    if (errors.length === 1) throw errors[0];
    if (errors.length > 1) {
      throw new AggregateError(errors, "failed to restore Vue client DOM globals");
    }
  };

  try {
    for (const key of CLIENT_DOM_GLOBAL_KEYS) {
      const value = key === "window" ? sharedDom.window : sharedDom.window[key];
      if (value === undefined) continue;
      const descriptor = Object.getOwnPropertyDescriptor(globalThis, key);
      previous.set(key, descriptor);
      Object.defineProperty(globalThis, key, {
        configurable: true,
        enumerable: descriptor?.enumerable ?? true,
        writable: true,
        value,
      });
      installed.push(key);
    }
  } catch (error) {
    try {
      restore();
    } catch (restoreError) {
      throw new AggregateError(
        [error, restoreError],
        "Vue client DOM global installation rollback failed",
      );
    }
    throw error;
  }
  return restore;
}

/** Every inline custom property (`--*`) actually set on `element`. */
function customPropertiesOf(element) {
  const properties = {};
  if (element === null || element === undefined) return properties;
  const { style } = element;
  for (let index = 0; index < style.length; index += 1) {
    const name = style.item(index);
    if (name.startsWith("--")) properties[name] = style.getPropertyValue(name).trim();
  }
  return properties;
}

/**
 * Mounts `moduleCode`'s default export as a CHILD of a host whose props are
 * the reactive `initialProps`, then hands the live mount to `drive`. Owns the
 * shared document, the global installation/restore, warning capture and
 * unmount, so every observation style reads one identical mount.
 *
 * @returns {Promise<{ error: string|null, warnings: string[] }>}
 */
async function withClientMount(moduleCode, initialProps, drive) {
  const runtimeHref = clientRuntimeHref();
  const filePath = scratchModulePath(redirectVueImports(moduleCode, runtimeHref));

  const warnings = [];
  let error = null;
  try {
    const sharedDom = await ensureClientDom();
    const restoreGlobals = installClientDomGlobals(sharedDom);
    const originalWarn = console.warn;
    console.warn = (...args) => {
      warnings.push(args.map(String).join(" "));
    };
    const container = sharedDom.window.document.createElement("div");
    sharedDom.window.document.body.appendChild(container);
    let app = null;
    try {
      const runtime = await import(runtimeHref);
      const mod = await import(pathToFileURL(filePath).href);
      const component = mod.default;
      const props = runtime.reactive({ ...initialProps });
      app = runtime.createApp({
        render: () => runtime.h(component, { ...props }),
      });
      app.config.warnHandler = (message) => warnings.push(message);
      app.mount(container);
      await drive({ runtime, props, container, window: sharedDom.window });
    } catch (mountError) {
      error = String(mountError?.stack ?? mountError);
    } finally {
      if (app !== null) {
        try {
          app.unmount();
        } catch {
          // An unmount failure must not mask the observation above.
        }
      }
      container.remove();
      console.warn = originalWarn;
      restoreGlobals();
    }
  } catch (loadError) {
    error = String(loadError?.stack ?? loadError);
  }
  return { error, warnings };
}

/**
 * Mounts a compiled CLIENT (vdom-backend) module through the pinned
 * official runtime in jsdom and observes the mounted root element across a
 * sequence of prop states.
 *
 * The module is rendered as a CHILD of a host whose props are reactive, so
 * a subsequent state is a genuine framework-level update (re-render +
 * post-flush effects) rather than a second mount. Each step is read after
 * `nextTick`, because the CSS-vars effect is a post-flush effect and has
 * not run yet when `mount()` returns.
 *
 * @param {string} moduleCode assembled module source (plain JS)
 * @param {{ propSteps?: Array<Record<string, unknown>> }} [options]
 * @returns {Promise<{ ok: boolean, error: string|null, warnings: string[],
 *   steps: Array<{ html: string, customProperties: Record<string, string> }> }>}
 */
export async function executeVueClientMount(moduleCode, options = {}) {
  const propSteps = options.propSteps ?? [{}];
  if (propSteps.length === 0) throw new Error("executeVueClientMount needs at least one prop step");
  const steps = [];
  const { error, warnings } = await withClientMount(
    moduleCode,
    propSteps[0],
    async ({ runtime, props, container }) => {
      for (const [index, step] of propSteps.entries()) {
        if (index > 0) {
          for (const key of Object.keys(props)) delete props[key];
          Object.assign(props, step);
        }
        await runtime.nextTick();
        steps.push({
          html: container.innerHTML,
          customProperties: customPropertiesOf(container.firstElementChild),
        });
      }
    },
  );
  return { ok: error === null, error, warnings, steps };
}

// ── Client interactions (runtime directive updates) ───────────────────
//
// A runtime directive (`v-show`, the native `v-model` family) applies its
// value from the `beforeUpdate`/`updated` hooks the patcher runs only for a
// VNode it actually revisits. Whether a re-render revisits the element is
// decided by the compiled output (its patch flag decides block tracking), and
// the failure is silent: the first render is right, the element simply stops
// following its value afterwards. Only driving real updates observes it.

function findInteractionTarget(container, selector) {
  const element = container.querySelector(selector);
  if (element === null) throw new Error(`interaction target ${selector} is not in the mounted DOM`);
  return element;
}

/**
 * Performs one user interaction with native event semantics: `click` runs
 * the element's activation behaviour (a checkbox/radio toggles its own
 * checkedness and dispatches `input` + `change` only when it changed), and
 * `select` picks an existing option and dispatches `change`.
 */
function performInteraction(container, window, action) {
  const element = findInteractionTarget(container, action.target);
  switch (action.kind) {
    case "click":
      element.click();
      return;
    case "select":
      element.value = action.value;
      // An unknown option would silently select nothing and read as a
      // no-op step instead of the interaction the case asked for.
      if (element.value !== action.value) {
        throw new Error(
          `${action.target} has no option with value ${JSON.stringify(action.value)}`,
        );
      }
      element.dispatchEvent(new window.Event("change", { bubbles: true }));
      return;
    default:
      throw new Error(`unknown interaction kind ${JSON.stringify(action.kind)}`);
  }
}

/**
 * Mounts a compiled CLIENT (vdom-backend) module through the pinned official
 * runtime in jsdom, performs `actions` in order, and observes the `observe`
 * selectors after mount and after each action (each read after `nextTick`,
 * so the re-render and its directive hooks have run).
 *
 * Per observed element and step: `node` is a stable identity (the same
 * number across steps means the same DOM node, not a replacement),
 * `display` the inline `style.display`, `checked`/`value` the live form
 * state (`null` where the element has none), `text` its text content, and
 * `attributeWrites` the attribute mutations the element received during that
 * step — a step whose bound values did not change must write nothing.
 *
 * @param {string} moduleCode assembled module source (plain JS)
 * @param {{ observe: string[], actions?: Array<{ kind: "click", target: string } |
 *   { kind: "select", target: string, value: string }> }} options
 * @returns {Promise<{ ok: boolean, error: string|null, warnings: string[],
 *   steps: Array<{ html: string, elements: Record<string, null | { node: number,
 *   display: string, checked: boolean|null, value: string|null, text: string,
 *   attributeWrites: number }>}> }>}
 */
export async function executeVueClientInteractions(moduleCode, options) {
  const { observe, actions = [] } = options;
  if (!Array.isArray(observe) || observe.length === 0) {
    throw new Error("executeVueClientInteractions needs at least one observed selector");
  }
  const steps = [];
  const { error, warnings } = await withClientMount(
    moduleCode,
    {},
    async ({ runtime, container, window }) => {
      const identities = new WeakMap();
      let nextIdentity = 0;
      const identityOf = (node) => {
        if (!identities.has(node)) identities.set(node, nextIdentity++);
        return identities.get(node);
      };
      const records = [];
      const observer = new window.MutationObserver((batch) => records.push(...batch));
      observer.observe(container, { subtree: true, childList: true, attributes: true });
      const snapshot = async () => {
        await runtime.nextTick();
        records.push(...observer.takeRecords());
        const elements = {};
        for (const selector of observe) {
          const element = container.querySelector(selector);
          elements[selector] =
            element === null
              ? null
              : {
                  node: identityOf(element),
                  display: element.style.display,
                  checked: typeof element.checked === "boolean" ? element.checked : null,
                  value: typeof element.value === "string" ? element.value : null,
                  text: element.textContent,
                  attributeWrites: records.filter(
                    (record) => record.type === "attributes" && record.target === element,
                  ).length,
                };
        }
        records.length = 0;
        steps.push({ html: container.innerHTML, elements });
      };
      try {
        await snapshot();
        for (const action of actions) {
          performInteraction(container, window, action);
          await snapshot();
        }
      } finally {
        observer.disconnect();
      }
    },
  );
  return { ok: error === null, error, warnings, steps };
}
