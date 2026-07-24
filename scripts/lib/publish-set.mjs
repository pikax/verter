#!/usr/bin/env node

/**
 * publish-set.mjs — the single authority for the npm publish set.
 *
 * The publish set is DERIVED from the shipped product, not hand-maintained:
 * starting from PRODUCT_ROOTS, walk the runtime dependency fields
 * (dependencies + optionalDependencies + peerDependencies), following only
 * workspace-resolvable packages. Anything a published package depends on at
 * runtime is itself published — automatically.
 *
 * Fails loud (throws) when the closure is unsatisfiable:
 *   - a package in the closure is marked `private: true`, or
 *   - a dependency cycle is detected.
 */

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "../..");

/** The shipped product. Everything publishable is derived from these roots. */
export const PRODUCT_ROOTS = [
  "@verter/typeinfo",
  "@verter/component-meta",
  "@verter/unplugin",
  "verter-tsc",
  "verter-vscode",
];

/** In the product, but published to the VS Code Marketplace only — never npm. */
export const MARKETPLACE_ONLY = ["verter-vscode"];

/** Runtime dependency fields. devDependencies do NOT propagate. */
const RUNTIME_DEP_FIELDS = ["dependencies", "optionalDependencies", "peerDependencies"];

function readJson(filePath) {
  return JSON.parse(readFileSync(filePath, "utf8"));
}

/**
 * Scan workspace packages: `packages/*` plus platform sub-packages under
 * `packages/<pkg>/npm/<platform>`. Returns Map<name, entry> where entry is
 * { name, dir, pkg, isPlatform }.
 */
export function scanWorkspacePackages(packagesDir) {
  const byName = new Map();

  const add = (dir, isPlatform) => {
    const pkgPath = join(dir, "package.json");
    if (!existsSync(pkgPath)) return;
    const pkg = readJson(pkgPath);
    byName.set(pkg.name, { name: pkg.name, dir, pkg, isPlatform });
  };

  for (const entry of readdirSync(packagesDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const dir = join(packagesDir, entry.name);
    add(dir, false);
    const npmDir = join(dir, "npm");
    if (!existsSync(npmDir)) continue;
    for (const sub of readdirSync(npmDir, { withFileTypes: true })) {
      if (sub.isDirectory()) add(join(npmDir, sub.name), true);
    }
  }

  return byName;
}

function runtimeDeps(entry) {
  const deps = [];
  for (const field of RUNTIME_DEP_FIELDS) {
    for (const depName of Object.keys(entry.pkg[field] ?? {})) {
      deps.push(depName);
    }
  }
  return deps;
}

/**
 * Topologically sort `names` so a dependency sorts before its dependents.
 * Throws on dependency cycles.
 */
function topoSort(names, closure) {
  const nameSet = new Set(names);
  const order = [];
  const state = new Map(); // 1 = visiting, 2 = done
  const stack = [];

  function visit(name) {
    const s = state.get(name);
    if (s === 2) return;
    if (s === 1) {
      const cycle = [...stack.slice(stack.indexOf(name)), name].join(" -> ");
      throw new Error(`publish-set: dependency cycle detected: ${cycle}`);
    }
    state.set(name, 1);
    stack.push(name);
    for (const depName of runtimeDeps(closure.get(name))) {
      if (nameSet.has(depName)) visit(depName);
    }
    stack.pop();
    state.set(name, 2);
    order.push(name);
  }

  for (const name of names) visit(name);
  return order;
}

/**
 * Compute the publish set from the product dependency closure.
 *
 * Returns {
 *   npm: [...names],             // publishable npm packages (marketplace-only excluded)
 *   order: [...names],           // npm set, topologically sorted (deps first)
 *   platform: [...paths],        // platform sub-package dirs, relative to rootDir
 *   marketplaceOnly: [...names], // product packages never published to npm
 * }
 */
export function computePublishSet(options = {}) {
  const rootDir = options.rootDir ?? ROOT;
  const roots = options.roots ?? PRODUCT_ROOTS;
  const workspace = scanWorkspacePackages(join(rootDir, "packages"));

  // BFS over runtime dep fields, following only workspace-resolvable names.
  const closure = new Map(); // name -> entry, in discovery order
  const queue = [];
  for (const root of roots) {
    const entry = workspace.get(root);
    if (!entry) {
      throw new Error(
        `publish-set: product root "${root}" is not a workspace package under packages/`,
      );
    }
    queue.push(entry);
  }
  while (queue.length > 0) {
    const entry = queue.shift();
    if (closure.has(entry.name)) continue;
    closure.set(entry.name, entry);
    for (const depName of runtimeDeps(entry)) {
      const dep = workspace.get(depName);
      if (dep) queue.push(dep);
    }
  }

  // Fail loud: a private package in the closure cannot be published, so the
  // closure is unsatisfiable — this is exactly the drift we are preventing.
  // Marketplace-only packages are exempt: they ship outside npm (e.g. the VS
  // Code Marketplace) and are expected to carry `private: true`.
  for (const entry of closure.values()) {
    if (entry.pkg.private && !MARKETPLACE_ONLY.includes(entry.name)) {
      throw new Error(
        `publish-set: "${entry.name}" is in the product dependency closure but is marked private — it cannot be published`,
      );
    }
  }

  // Platform sub-packages: those reached via optionalDependencies, plus any
  // <pkg>/npm/<platform> dir present on disk for a closure package.
  const platform = new Set();
  for (const entry of closure.values()) {
    if (entry.isPlatform) {
      platform.add(relative(rootDir, entry.dir));
      continue;
    }
    const npmDir = join(entry.dir, "npm");
    if (!existsSync(npmDir)) continue;
    for (const sub of readdirSync(npmDir, { withFileTypes: true })) {
      if (sub.isDirectory() && existsSync(join(npmDir, sub.name, "package.json"))) {
        platform.add(relative(rootDir, join(npmDir, sub.name)));
      }
    }
  }

  const marketplaceOnly = MARKETPLACE_ONLY.filter((name) => closure.has(name));
  const npm = [...closure.values()]
    .filter((entry) => !entry.isPlatform && !MARKETPLACE_ONLY.includes(entry.name))
    .map((entry) => entry.name);
  const order = topoSort(npm, closure);

  return { npm, order, platform: [...platform].sort(), marketplaceOnly };
}
