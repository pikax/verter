/**
 * The HERMETIC engine assets for the in-context LanguageService worker: the
 * pinned local `typescript@6` `lib.*.d.ts` set plus the pinned local Vue type
 * packages, statically bundled by Vite (`?raw` / glob) from this repo's
 * `node_modules` — NEVER a CDN. IndexedDB is a runtime CACHE only, keyed by
 * the TS version string, so an engine bump can never serve stale libs.
 *
 * Everything is addressed into a virtual `/node_modules` tree the worker's
 * layered file system serves.
 */
import vueDts from "../../node_modules/vue/dist/vue.d.ts?raw";
import vueJsxDts from "../../node_modules/vue/jsx.d.ts?raw";
import vueJsxRuntimeDts from "../../node_modules/vue/jsx-runtime/index.d.ts?raw";
import runtimeDomDts from "../../node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts?raw";
import runtimeCoreDts from "../../node_modules/@vue/runtime-core/dist/runtime-core.d.ts?raw";
import reactivityDts from "../../node_modules/@vue/reactivity/dist/reactivity.d.ts?raw";
import sharedDts from "../../node_modules/@vue/shared/dist/shared.d.ts?raw";
import csstypeDts from "../../node_modules/csstype/index.d.ts?raw";

/** The virtual lib directory the worker host serves default libs from. */
export const WORKER_LIB_DIR = "/node_modules/typescript/lib";

/** The vue `jsx.d.ts` global-JSX registration — a program ROOT in the worker. */
export const VUE_JSX_GLOBAL_PATH = "/node_modules/vue/jsx.d.ts";

/**
 * The pinned engine's full `lib.*.d.ts` set, bundled at build time from the
 * SAME package the worker's `typescript` import resolves to (the playground's
 * direct devDependency) — engine and libs cannot skew.
 */
const TS_LIBS = import.meta.glob("../../node_modules/typescript/lib/lib.*.d.ts", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Minimal package manifest for classic `types`-field resolution. */
function packageJson(name: string, types: string): string {
  return JSON.stringify({ name, version: "0.0.0-bundled", types });
}

/**
 * The complete bundled virtual-file tree: TS libs + Vue (+ transitive
 * `@vue/*`, `csstype`) type packages, keyed by virtual absolute path.
 */
export function bundledWorkerFiles(): Map<string, string> {
  const files = new Map<string, string>();
  for (const [modulePath, content] of Object.entries(TS_LIBS)) {
    const name = modulePath.slice(modulePath.lastIndexOf("/") + 1);
    files.set(`${WORKER_LIB_DIR}/${name}`, content);
  }

  files.set("/node_modules/vue/package.json", packageJson("vue", "dist/vue.d.ts"));
  files.set("/node_modules/vue/dist/vue.d.ts", vueDts);
  files.set(VUE_JSX_GLOBAL_PATH, vueJsxDts);
  files.set(
    "/node_modules/vue/jsx-runtime/package.json",
    packageJson("vue-jsx-runtime", "index.d.ts"),
  );
  files.set("/node_modules/vue/jsx-runtime/index.d.ts", vueJsxRuntimeDts);

  files.set(
    "/node_modules/@vue/runtime-dom/package.json",
    packageJson("@vue/runtime-dom", "dist/runtime-dom.d.ts"),
  );
  files.set("/node_modules/@vue/runtime-dom/dist/runtime-dom.d.ts", runtimeDomDts);
  files.set(
    "/node_modules/@vue/runtime-core/package.json",
    packageJson("@vue/runtime-core", "dist/runtime-core.d.ts"),
  );
  files.set("/node_modules/@vue/runtime-core/dist/runtime-core.d.ts", runtimeCoreDts);
  files.set(
    "/node_modules/@vue/reactivity/package.json",
    packageJson("@vue/reactivity", "dist/reactivity.d.ts"),
  );
  files.set("/node_modules/@vue/reactivity/dist/reactivity.d.ts", reactivityDts);
  files.set(
    "/node_modules/@vue/shared/package.json",
    packageJson("@vue/shared", "dist/shared.d.ts"),
  );
  files.set("/node_modules/@vue/shared/dist/shared.d.ts", sharedDts);
  files.set("/node_modules/csstype/package.json", packageJson("csstype", "index.d.ts"));
  files.set("/node_modules/csstype/index.d.ts", csstypeDts);

  return files;
}

// ── IndexedDB runtime cache (version-keyed; never a source of truth) ──

const DB_NAME = "verter-ts-libs";
const DB_VERSION = 2;
const STORE_NAME = "libs";

/**
 * The cache key for one lib/type asset: ALWAYS prefixed by the exact engine
 * version string, so a TypeScript upgrade invalidates every entry wholesale.
 */
export function libCacheKey(tsVersion: string, name: string): string {
  return `ts@${tsVersion}:${name}`;
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      if (!req.result.objectStoreNames.contains(STORE_NAME)) {
        req.result.createObjectStore(STORE_NAME);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

/**
 * Best-effort write-through of the bundled assets into the version-keyed
 * runtime cache. Purely an optimization surface (e.g. future offline reuse);
 * the BUNDLE stays authoritative and a failure here is non-fatal.
 */
export async function seedLibCache(tsVersion: string, files: Map<string, string>): Promise<void> {
  if (typeof indexedDB === "undefined") return;
  try {
    const db = await openDb();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(STORE_NAME, "readwrite");
      const store = tx.objectStore(STORE_NAME);
      for (const [path, content] of files) {
        store.put(content, libCacheKey(tsVersion, path));
      }
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
    db.close();
  } catch {
    // Cache write failure is non-fatal — the bundle is authoritative.
  }
}
