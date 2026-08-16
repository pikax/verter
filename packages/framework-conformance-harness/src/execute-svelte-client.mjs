// Deterministic execution of compiled Svelte CLIENT output against the pinned
// OFFICIAL client runtime — never a Verter-owned runtime. The sibling
// `execute-svelte-runtime.mjs` executes only `generate: "server"` output
// through `svelte/server`'s `render`; this module is its client half, so a
// `generate: "client"` artifact is runtime-observable too.
//
// Runtime and compiled scratch module both load from the isolated per-domain
// installation (oracle-install.mjs), so bare `svelte/*` imports resolve against
// the exact realized closure and share one module instance graph.
//
// The pinned package's `.` export resolves to its SERVER build under Node (its
// `exports` map lists `"browser": "./src/index-client.js"` and
// `"default": "./src/index-server.js"`), and the server build's `mount` throws
// `lifecycle_function_unavailable`. The client entry is therefore imported by
// its resolved path inside the same install — still the pinned closure, just
// the browser condition of it.

import { createHash, randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { ensureOracleDomain, oracleScratchDir } from "./oracle-install.mjs";

const SCRATCH_LABEL = `svelte-client-${randomUUID()}`;
let scratchDir = null;
let sharedDom = null;
let restoreSharedGlobals = null;
let sharedRuntimePromise = null;

/**
 * A compiled client module reaches its runtime through a BARE
 * `svelte/internal/client` specifier, which Node resolves by walking UP from
 * the module's own directory. Relying on that walk to arrive at the pinned
 * install is not resolution — it is a coincidence of where the scratch
 * directory happens to sit, and any `svelte` in an ancestor `node_modules`
 * wins instead. When it does, the compiled module binds a SECOND runtime
 * instance whose `init_operations()` never ran, and the mount dies inside the
 * official runtime with `Cannot read properties of undefined (reading 'call')`
 * at `get_first_child` — an opaque failure that looks like a candidate defect.
 *
 * So the walk is terminated at its FIRST step: a `node_modules` link inside the
 * scratch directory pointing at the pinned install's own `node_modules`. No
 * ancestor can be consulted, because resolution succeeds before the walk
 * begins. `junction` is the portable directory-link type — Windows needs it,
 * and POSIX treats it as a directory symlink.
 */
function ensureScratchDir(installDir) {
  if (scratchDir !== null) return scratchDir;
  scratchDir = oracleScratchDir("svelte", SCRATCH_LABEL);
  mkdirSync(scratchDir, { recursive: true });
  const link = path.join(scratchDir, "node_modules");
  if (!existsSync(link)) {
    symlinkSync(path.join(installDir, "node_modules"), link, "junction");
  }
  return scratchDir;
}

function scratchModulePath(code, installDir) {
  const dir = ensureScratchDir(installDir);
  const digest = createHash("sha256").update(code).digest("hex").slice(0, 16);
  const filePath = path.join(dir, `svelte-client-${digest}.mjs`);
  writeFileSync(filePath, code, "utf8");
  return filePath;
}

/**
 * Resolve, from the scratch module's own position, the package a bare `svelte`
 * specifier reaches — the same resolution the compiled module performs — and
 * report its path and version.
 *
 * @param {string} fromFile a file inside the scratch directory
 * @returns {{ packageDir: string, version: string }}
 */
export function resolveBoundRuntime(fromFile) {
  const manifestPath = createRequire(fromFile).resolve("svelte/package.json");
  const version = JSON.parse(readFileSync(manifestPath, "utf8")).version;
  return { packageDir: path.dirname(manifestPath), version };
}

/** The DOM globals the pinned client runtime reads after initialization. */
const DOM_GLOBAL_KEYS = [
  "document",
  "HTMLElement",
  "Node",
  "Element",
  "Text",
  "Comment",
  "DocumentFragment",
  "customElements",
  "requestAnimationFrame",
  "cancelAnimationFrame",
  "Event",
  "CustomEvent",
  "MutationObserver",
  "getComputedStyle",
];

export class SvelteClientDomInitializationError extends Error {
  constructor(message, options) {
    super(`Svelte client DOM initialization failed: ${message}`, options);
    this.name = "SvelteClientDomInitializationError";
  }
}

function collectCleanupErrors(steps) {
  const errors = [];
  for (const step of steps) {
    try {
      step();
    } catch (error) {
      errors.push(error);
    }
  }
  return errors;
}

function throwCleanupErrors(errors, message) {
  if (errors.length === 1) throw errors[0];
  if (errors.length > 1) throw new AggregateError(errors, message);
}

/**
 * Install JSDOM's navigator without assigning through Node's getter-only
 * global. A configurable property is replaced and restored exactly; a
 * non-configurable navigator is preserved only when it supplies the value the
 * runtime reads.
 */
function installNavigator(dom) {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  if (previous?.configurable === false) {
    let current;
    try {
      current = globalThis.navigator;
    } catch (error) {
      throw new SvelteClientDomInitializationError(
        "the existing non-configurable navigator cannot be read",
        { cause: error },
      );
    }
    if (typeof current?.userAgent !== "string") {
      throw new SvelteClientDomInitializationError(
        "the existing non-configurable navigator has no string userAgent",
      );
    }
    return () => {};
  }

  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    enumerable: previous?.enumerable ?? true,
    writable: true,
    value: dom.window.navigator,
  });
  return () => {
    if (previous === undefined) delete globalThis.navigator;
    else Object.defineProperty(globalThis, "navigator", previous);
  };
}

function installDomGlobals(dom) {
  const previous = new Map([["window", globalThis.window]]);
  globalThis.window = dom.window;
  for (const key of DOM_GLOBAL_KEYS) {
    previous.set(key, globalThis[key]);
    if (dom.window[key] !== undefined) globalThis[key] = dom.window[key];
  }
  const restoreValues = () => {
    for (const [key, value] of previous) {
      if (value === undefined) delete globalThis[key];
      else globalThis[key] = value;
    }
  };
  let restoreNavigator;
  try {
    restoreNavigator = installNavigator(dom);
  } catch (error) {
    restoreValues();
    throw error;
  }
  return () => {
    const errors = collectCleanupErrors([restoreValues, restoreNavigator]);
    throwCleanupErrors(errors, "failed to restore Svelte client DOM globals");
  };
}

function assertNavigatorReady(stage) {
  let navigatorValue;
  try {
    navigatorValue = globalThis.navigator;
  } catch (error) {
    throw new SvelteClientDomInitializationError(`navigator cannot be read ${stage}`, {
      cause: error,
    });
  }
  if (typeof navigatorValue?.userAgent !== "string") {
    throw new SvelteClientDomInitializationError(`navigator.userAgent is unavailable ${stage}`);
  }
}

async function initializeSharedRuntime(installDir) {
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    pretendToBeVisual: true,
  });
  let restore = null;
  try {
    restore = installDomGlobals(dom);
    assertNavigatorReady("before importing the client runtime");
    const clientEntry = path.join(installDir, "node_modules", "svelte", "src", "index-client.js");
    const runtime = await import(pathToFileURL(clientEntry).href);
    sharedDom = dom;
    restoreSharedGlobals = restore;
    return { dom, runtime };
  } catch (error) {
    const cleanupErrors = collectCleanupErrors([() => restore?.(), () => dom.window.close()]);
    if (error instanceof SvelteClientDomInitializationError && cleanupErrors.length === 0) {
      throw error;
    }
    const cause =
      cleanupErrors.length === 0
        ? error
        : new AggregateError(
            [error, ...cleanupErrors],
            "client runtime initialization cleanup failed",
          );
    throw new SvelteClientDomInitializationError("the shared client runtime could not initialize", {
      cause,
    });
  }
}

function sharedRuntime(installDir) {
  if (sharedRuntimePromise === null) {
    const pending = initializeSharedRuntime(installDir).catch((error) => {
      if (sharedRuntimePromise === pending) {
        sharedRuntimePromise = null;
        const dom = sharedDom;
        const restore = restoreSharedGlobals;
        sharedDom = null;
        restoreSharedGlobals = null;
        const cleanupErrors = collectCleanupErrors([() => restore?.(), () => dom?.window.close()]);
        if (cleanupErrors.length > 0) {
          throw new SvelteClientDomInitializationError(
            "the rejected shared client runtime could not be cleaned up",
            {
              cause: new AggregateError([error, ...cleanupErrors]),
            },
          );
        }
      }
      throw error;
    });
    sharedRuntimePromise = pending;
  }
  return sharedRuntimePromise;
}

/**
 * Mounts one compiled CLIENT module and returns the rendered markup.
 *
 * @param {string} clientCode module source compiled with generate:"client"
 * @param {object} props
 * @returns {Promise<{ ok: boolean, html: string|null, error: string|null }>}
 */
export async function executeSvelteClient(clientCode, props = {}) {
  const { installDir } = ensureOracleDomain("svelte");
  const filePath = scratchModulePath(clientCode, installDir);

  // FAIL CLOSED before mounting: the module's own bare specifier must reach the
  // pinned install, and it must be the same package the executor imports
  // `mount` from. A mismatch is reported as itself, never left to surface as an
  // opaque runtime TypeError that reads like a candidate defect.
  const bound = resolveBoundRuntime(filePath);
  const pinnedPackageDir = path.join(installDir, "node_modules", "svelte");
  if (path.resolve(bound.packageDir) !== path.resolve(pinnedPackageDir)) {
    return {
      ok: false,
      html: null,
      error:
        `the compiled module's bare \`svelte\` specifier resolves to ${bound.packageDir} ` +
        `(v${bound.version}), not the pinned install at ${pinnedPackageDir}; mounting it would ` +
        `bind a second runtime instance`,
      runtime: bound,
    };
  }

  let target = null;
  try {
    const { dom, runtime } = await sharedRuntime(installDir);
    assertNavigatorReady("before the client runtime's first operation");
    const { mount, unmount, flushSync } = runtime;
    const module_ = await import(pathToFileURL(filePath).href);
    target = dom.window.document.createElement("div");
    dom.window.document.body.appendChild(target);
    const instance = mount(module_.default, { target, props });
    flushSync();
    const html = target.innerHTML;
    unmount(instance);
    flushSync();
    return { ok: true, html, error: null, runtime: bound };
  } catch (error) {
    return { ok: false, html: null, error: String(error?.stack ?? error), runtime: bound };
  } finally {
    target?.remove();
  }
}

export function cleanupClientScratch() {
  const dir = scratchDir;
  const restore = restoreSharedGlobals;
  const dom = sharedDom;
  scratchDir = null;
  sharedRuntimePromise = null;
  restoreSharedGlobals = null;
  sharedDom = null;
  const errors = collectCleanupErrors([
    () => {
      if (dir !== null) rmSync(dir, { recursive: true, force: true });
    },
    () => restore?.(),
    () => dom?.window.close(),
  ]);
  throwCleanupErrors(errors, "failed to clean up Svelte client scratch state");
}
