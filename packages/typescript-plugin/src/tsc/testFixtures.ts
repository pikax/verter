// Hermetic temp-project fixture builders for the batch-typecheck driver tests.
//
// A fixture is a fresh OS-temp project whose `node_modules` symlinks ONLY
// workspace-vendored packages (`@verter/types`, `vue`) — no third-party repo
// checkout (the testing-hermeticity rule). The `.vue`/`.svelte` carriers the
// driver materialises need `@verter/types` (IDE-carrier helper imports) and
// `vue` (declaration-carrier base types) resolvable, so the fixture wires both.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/** Repository root (the worktree). Derived from this file's location. */
export const WORKSPACE_ROOT = path.resolve(__dirname, "..", "..", "..", "..");

/** The built `@verter/types` package dir (must be built before the tests run). */
export const VERTER_TYPES_DIR = path.join(WORKSPACE_ROOT, "packages", "types");

/** The `@verter/svelte-jsx` package dir (the Svelte carrier's JSX namespace). */
export const VERTER_SVELTE_JSX_DIR = path.join(WORKSPACE_ROOT, "packages", "svelte-jsx");

/**
 * Find a built `vue` package in the workspace pnpm store. Picks the 3.5.x line
 * (the workspace's pinned `@vue/compiler-sfc` 3.5.34 baseline) when present.
 */
export function resolveWorkspaceVueDir(): string {
  const pnpmDir = path.join(WORKSPACE_ROOT, "node_modules", ".pnpm");
  const entries = fs.existsSync(pnpmDir) ? fs.readdirSync(pnpmDir) : [];
  const candidates = entries.filter((e) => /^vue@3\.5\.\d+_typescript@/.test(e));
  const chosen = candidates.sort().reverse()[0];
  if (chosen === undefined) {
    throw new Error(
      "batch-tsc fixtures: no vue@3.5.x in the workspace pnpm store — run `pnpm install`",
    );
  }
  return path.join(pnpmDir, chosen, "node_modules");
}

/**
 * The compiler options a real Verter + Vue project uses so the generated IDE /
 * declaration carriers type-check faithfully:
 * - `jsx: "preserve"` + `jsxImportSource: "vue"` — the IDE carrier's template
 *   lowering emits JSX whose intrinsics come from Vue's JSX runtime (without it
 *   every element is `TS7026 JSX.IntrinsicElements`).
 * - the DOM libs — template elements reference DOM types.
 * Mirrors `packages/vue-vscode/e2e/fixtures/external-ts-engine/tsconfig.json`.
 */
export function verterProjectConfig(overrides?: Record<string, unknown>): Record<string, unknown> {
  return {
    compilerOptions: {
      target: "ESNext",
      module: "ESNext",
      moduleResolution: "bundler",
      jsx: "preserve",
      jsxImportSource: "vue",
      strict: true,
      noEmit: true,
      skipLibCheck: true,
      esModuleInterop: true,
      lib: ["ESNext", "DOM", "DOM.Iterable"],
      types: [],
      ...overrides,
    },
    include: ["src"],
  };
}

/** A created temp fixture project. */
export interface Fixture {
  /** Absolute path of the project root. */
  root: string;
  /** Absolute path of the project's `tsconfig.json`. */
  tsconfigPath: string;
  /** Remove the fixture tree (call in `afterEach`). */
  cleanup(): void;
}

/** A `.vue`/`.svelte` source file to materialise in a fixture. */
export interface FixtureSource {
  /** Path relative to the project root (e.g. `src/Foo.vue`). */
  rel: string;
  content: string;
}

/**
 * Symlink `@verter/types` and the workspace `vue` (with its transitive `@vue/*`
 * deps) into `<root>/node_modules` so a carrier referencing them resolves
 * without a real install. Uses Windows `junction` links (which need no admin
 * rights) so the fixtures are cross-platform.
 */
function wireNodeModules(root: string): void {
  const nm = path.join(root, "node_modules");
  fs.mkdirSync(path.join(nm, "@verter"), { recursive: true });

  link(VERTER_TYPES_DIR, path.join(nm, "@verter", "types"));
  // The Svelte carrier emits `/** @jsxImportSource @verter/svelte-jsx */`.
  link(VERTER_SVELTE_JSX_DIR, path.join(nm, "@verter", "svelte-jsx"));

  const vuePnpmNm = resolveWorkspaceVueDir();
  // Link the carrier-runtime package and every sibling dep it re-exports
  // (`@vue/runtime-dom`, etc.) so the declaration carrier's `import("vue")`
  // resolves its full type graph. `link()` is idempotent, so re-linking the
  // primary package below is a no-op.
  for (const dep of fs.readdirSync(vuePnpmNm)) {
    if (dep.startsWith("@")) {
      const scopeDir = path.join(nm, dep);
      fs.mkdirSync(scopeDir, { recursive: true });
      for (const sub of fs.readdirSync(path.join(vuePnpmNm, dep))) {
        link(path.join(vuePnpmNm, dep, sub), path.join(scopeDir, sub));
      }
    } else {
      link(path.join(vuePnpmNm, dep), path.join(nm, dep));
    }
  }
}

/** Create a directory junction/symlink, tolerating a pre-existing link. */
function link(target: string, linkPath: string): void {
  if (fs.existsSync(linkPath)) return;
  try {
    fs.symlinkSync(target, linkPath, "junction");
  } catch {
    // A junction can fail for a non-directory target; fall back to a dir copy is
    // unnecessary here (all targets are dirs). Swallow — a missing optional dep
    // surfaces as a normal resolution diagnostic.
  }
}

/**
 * Create a single-project fixture (no project references). `tsconfigOptions`
 * overrides/extends the default compiler options.
 */
export function createSingleProjectFixture(
  sources: FixtureSource[],
  tsconfigOptions?: Record<string, unknown>,
): Fixture {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "verter-batch-tsc-"));
  fs.mkdirSync(path.join(root, "src"), { recursive: true });
  wireNodeModules(root);

  for (const s of sources) {
    const abs = path.join(root, s.rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, s.content, "utf8");
  }

  const tsconfigPath = path.join(root, "tsconfig.json");
  fs.writeFileSync(
    tsconfigPath,
    JSON.stringify(verterProjectConfig(tsconfigOptions), null, 2),
    "utf8",
  );

  return {
    root,
    tsconfigPath,
    cleanup() {
      fs.rmSync(root, { recursive: true, force: true });
    },
  };
}

/** A project-reference graph fixture: a referenced leaf + a referencing root. */
export interface ReferenceFixture extends Fixture {
  /** Absolute path of the referencing (leaf-under-check) project's tsconfig. */
  appTsconfigPath: string;
  /** Absolute path of the referenced (library) project's tsconfig. */
  libTsconfigPath: string;
}

/**
 * Create a `composite` + project-reference + `paths` fixture:
 * - `packages/lib` — a referenced `composite` project exporting a `.vue`/`.svelte`
 *   component.
 * - `packages/app` — the referencing project that imports the lib component via a
 *   `paths` alias and references the lib project.
 *
 * This is the §2.4 spike shape: build mode must emit the lib's `.d.ts` into the
 * mirror and the app must resolve the lib component through it.
 */
export function createReferenceFixture(args: {
  libSources: FixtureSource[];
  appSources: FixtureSource[];
  /** `paths` alias map applied to the APP config (e.g. `{ "@lib/*": ["../lib/src/*"] }`). */
  appPaths?: Record<string, string[]>;
}): ReferenceFixture {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "verter-batch-tsc-ref-"));
  const libRoot = path.join(root, "packages", "lib");
  const appRoot = path.join(root, "packages", "app");
  fs.mkdirSync(path.join(libRoot, "src"), { recursive: true });
  fs.mkdirSync(path.join(appRoot, "src"), { recursive: true });
  wireNodeModules(root);

  for (const s of args.libSources) {
    const abs = path.join(libRoot, s.rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, s.content, "utf8");
  }
  for (const s of args.appSources) {
    const abs = path.join(appRoot, s.rel);
    fs.mkdirSync(path.dirname(abs), { recursive: true });
    fs.writeFileSync(abs, s.content, "utf8");
  }

  const libTsconfigPath = path.join(libRoot, "tsconfig.json");
  const libConfig = verterProjectConfig({
    composite: true,
    declaration: true,
    noEmit: false,
    outDir: "./dist",
    rootDir: "./src",
  });
  fs.writeFileSync(libTsconfigPath, JSON.stringify(libConfig, null, 2), "utf8");

  const appTsconfigPath = path.join(appRoot, "tsconfig.json");
  const appConfig = verterProjectConfig({
    baseUrl: ".",
    paths: args.appPaths ?? {},
  });
  (appConfig as { references?: unknown }).references = [{ path: "../lib" }];
  fs.writeFileSync(appTsconfigPath, JSON.stringify(appConfig, null, 2), "utf8");

  return {
    root,
    tsconfigPath: appTsconfigPath,
    appTsconfigPath,
    libTsconfigPath,
    cleanup() {
      fs.rmSync(root, { recursive: true, force: true });
    },
  };
}

/**
 * Snapshot every file under `dir` (EXCLUDING `node_modules`), keyed by relative
 * path → content. Used by the zero-working-tree-writes guard to compare the user
 * tree before/after a batch run.
 */
export function snapshotTree(dir: string): Map<string, string> {
  const acc = new Map<string, string>();
  walk(dir, dir, acc);
  return acc;
}

function walk(dir: string, base: string, acc: Map<string, string>): void {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(full, base, acc);
    } else if (entry.isFile()) {
      acc.set(path.relative(base, full).replace(/\\/g, "/"), fs.readFileSync(full, "utf8"));
    }
  }
}

/** Diff two tree snapshots → the relative paths that were added/modified/removed. */
export function diffTrees(
  before: Map<string, string>,
  after: Map<string, string>,
): { added: string[]; modified: string[]; removed: string[] } {
  const added: string[] = [];
  const modified: string[] = [];
  const removed: string[] = [];
  for (const [k, v] of after) {
    if (!before.has(k)) added.push(k);
    else if (before.get(k) !== v) modified.push(k);
  }
  for (const k of before.keys()) {
    if (!after.has(k)) removed.push(k);
  }
  return { added, modified, removed };
}
