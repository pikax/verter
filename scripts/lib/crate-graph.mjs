#!/usr/bin/env node

/**
 * crate-graph.mjs — pure functions over `cargo metadata` output: build the
 * workspace package index, the reverse-dependency graph, and map a changed
 * file path to the crate that owns it.
 *
 * Nothing here shells out or touches the filesystem beyond what the caller
 * hands it (a parsed `cargo metadata --format-version=1` document). That
 * keeps this module unit-testable against small synthetic fixtures instead
 * of the real ~40-crate workspace graph.
 */

/**
 * Build the workspace package index from a parsed `cargo metadata` document.
 *
 * @param {object} metadata - parsed `cargo metadata --format-version=1` JSON
 * @returns {{
 *   packages: Map<string, {id: string, name: string, manifestDir: string, isProcMacro: boolean}>,
 *   byName: Map<string, {id: string, name: string, manifestDir: string, isProcMacro: boolean}>,
 *   memberIds: Set<string>,
 * }}
 */
export function buildWorkspaceIndex(metadata) {
  const memberIds = new Set(metadata.workspace_members);
  const workspaceRoot = normalizeSlashes(metadata.workspace_root);
  const packages = new Map();
  const byName = new Map();
  for (const pkg of metadata.packages) {
    if (!memberIds.has(pkg.id)) continue;
    const manifestPath = normalizeSlashes(pkg.manifest_path);
    const manifestDir = relativeToRoot(workspaceRoot, dirnamePosix(manifestPath));
    const isProcMacro = pkg.targets.some((t) => t.kind.includes("proc-macro"));
    const entry = { id: pkg.id, name: pkg.name, manifestDir, isProcMacro };
    packages.set(pkg.id, entry);
    byName.set(pkg.name, entry);
  }
  return { packages, byName, memberIds, workspaceRoot };
}

function normalizeSlashes(p) {
  return p.replace(/\\/g, "/");
}

function dirnamePosix(p) {
  const idx = p.lastIndexOf("/");
  return idx === -1 ? "" : p.slice(0, idx);
}

function relativeToRoot(root, absDir) {
  if (absDir === root) return "";
  const prefix = root.endsWith("/") ? root : `${root}/`;
  if (absDir.startsWith(prefix)) return absDir.slice(prefix.length);
  // Not under the workspace root at all — return as-is; callers treat an
  // unmapped result as "does not match any crate" rather than crashing.
  return absDir;
}

/**
 * Build the reverse-dependency graph: for each workspace crate, the set of
 * OTHER workspace crates that directly depend on it (normal, build, AND dev
 * dependency edges all count — a dev-dependency is exactly the shape of the
 * class this tool must catch: crate B's test suite depends on crate A even
 * though B's *library* never links A).
 *
 * @param {object} metadata - parsed `cargo metadata` JSON
 * @param {ReturnType<typeof buildWorkspaceIndex>} index
 * @returns {Map<string, Set<string>>} depName -> Set(names of crates that depend on it)
 */
export function buildReverseDependencyGraph(metadata, index) {
  const idToName = new Map(metadata.packages.map((p) => [p.id, p.name]));
  const reverse = new Map();
  for (const pkg of index.packages.values()) reverse.set(pkg.name, new Set());

  for (const node of metadata.resolve.nodes) {
    if (!index.memberIds.has(node.id)) continue;
    const dependentName = idToName.get(node.id);
    for (const dep of node.deps) {
      if (!index.memberIds.has(dep.pkg)) continue; // external crate — not our concern
      const depName = idToName.get(dep.pkg);
      if (depName === dependentName) continue;
      if (!reverse.has(depName)) reverse.set(depName, new Set());
      reverse.get(depName).add(dependentName);
    }
  }
  return reverse;
}

/**
 * BFS the reverse-dependency graph from a set of starting crate names,
 * returning the full transitive closure (start crates included).
 *
 * @param {Map<string, Set<string>>} reverseGraph
 * @param {Iterable<string>} startNames
 * @returns {Set<string>}
 */
export function transitiveDependents(reverseGraph, startNames) {
  const seen = new Set(startNames);
  const queue = [...startNames];
  while (queue.length > 0) {
    const current = queue.shift();
    const dependents = reverseGraph.get(current);
    if (!dependents) continue;
    for (const dependent of dependents) {
      if (!seen.has(dependent)) {
        seen.add(dependent);
        queue.push(dependent);
      }
    }
  }
  return seen;
}

/**
 * Map a workspace-relative changed-file path to the crate that owns it, by
 * longest-matching manifest-directory PATH-SEGMENT prefix (not a naive
 * string prefix — `crates/verter_no_storedspan` must not match a file under
 * the sibling `crates/verter_no_storedspan_derive/`).
 *
 * @param {ReturnType<typeof buildWorkspaceIndex>} index
 * @param {string} relPath - forward-slash, workspace-root-relative path
 * @returns {{id: string, name: string, manifestDir: string, isProcMacro: boolean} | null}
 */
export function mapPathToCrate(index, relPath) {
  let best = null;
  for (const pkg of index.packages.values()) {
    const dir = pkg.manifestDir;
    const matches = dir === "" ? true : relPath === dir || relPath.startsWith(`${dir}/`);
    if (!matches) continue;
    if (!best || dir.length > best.manifestDir.length) best = pkg;
  }
  return best;
}

/**
 * The exact escape-hatch category set. Each rule fires a fallback to the
 * FULL workspace test set — see `matchEscapeHatch` and the rationale for
 * each category in `affected-tests.mjs`'s `--help` text (kept in one place
 * so `--help` cannot drift from the enforced behavior).
 */
export const GENERATED_BINDING_FILES = new Set([
  "packages/types/audit.generated.ts",
  "packages/language-shared/src/client-framework-manifest.generated.ts",
  "packages/language-shared/src/virtual-file-naming.generated.ts",
]);

export const ESCAPE_HATCH_RULES = [
  {
    id: "workspace-manifest",
    reason:
      "workspace root Cargo.toml/Cargo.lock changes dependency resolution and feature unification for every crate",
    test: (p) => p === "Cargo.toml" || p === "Cargo.lock",
  },
  {
    id: "nextest-config",
    reason:
      "nextest.toml controls test execution semantics (profiles, retries, filters) for the whole workspace run",
    test: (p) => p === ".config/nextest.toml",
  },
  {
    id: "scripts-tooling",
    reason:
      "scripts/ is build/gate/CI tooling, including this selector's own source — a change here can invalidate the selection logic itself",
    test: (p) => p === "scripts" || p.startsWith("scripts/"),
  },
  {
    id: "verter-identity",
    reason:
      "verter_identity is explicitly foundational — every crate's identity semantics can depend on it in ways the dependency graph alone may not fully capture",
    test: (p) => p === "crates/verter_identity" || p.startsWith("crates/verter_identity/"),
  },
  {
    id: "generated-bindings",
    reason:
      "a byte-pinned generated file (or its proto source); the freshness guard for it lives in a crate whose directory doesn't textually reference this path, so directory-mapping alone would miss the dependency",
    test: (p) => GENERATED_BINDING_FILES.has(p) || p.startsWith("crates/verter_protocol/proto/"),
  },
  {
    id: "proc-macro-crate",
    reason:
      "a proc-macro crate's blast radius is build-time token expansion into every consumer, not a normal linking edge the reverse-dependency graph models faithfully",
    test: (p, index) => {
      const pkg = mapPathToCrate(index, p);
      return Boolean(pkg && pkg.isProcMacro);
    },
  },
  {
    id: "ci-workflows",
    reason:
      "CI workflow definitions decide what actually runs at landing time; a selector cannot reason about pipeline-level changes",
    test: (p) => p === ".github/workflows" || p.startsWith(".github/workflows/"),
  },
];

/**
 * @param {string} relPath
 * @param {ReturnType<typeof buildWorkspaceIndex>} index
 * @returns {{id: string, reason: string} | null}
 */
export function matchEscapeHatch(relPath, index) {
  for (const rule of ESCAPE_HATCH_RULES) {
    if (rule.test(relPath, index)) return { id: rule.id, reason: rule.reason };
  }
  return null;
}

/**
 * Top-level paths known to never affect Rust test correctness — editor
 * config, docs, non-Rust tooling directories, and root metadata files with
 * no bearing on the Rust build. Anything else at the top level that isn't a
 * workspace-member root (`crates/*`, `xtask/`) and isn't matched by an
 * escape hatch falls through to the FULL workspace as an "unrecognized
 * top-level path" (over-select, never under-select).
 */
export const KNOWN_NON_RUST_TOP_LEVEL = new Set([
  ".analysis",
  ".claude",
  ".github", // .github/workflows/** is its own escape hatch, checked first
  ".husky",
  ".vscode",
  "docs",
  "editors",
  "examples",
  "extensions",
  "mcp",
  "packages", // generated-bindings files under here are their own escape hatch
  "schemas",
  "test-corpora",
  "test-results",
  "tools",
  "readme.md",
  "CHANGELOG.md",
  "CONTRIBUTING.md",
  "AGENTS.md",
  "LICENSE",
  ".gitattributes",
  ".gitignore",
  ".npmrc",
  ".nvmrc",
  ".lintstagedrc.cjs",
  "netlify.toml",
  "cliff.toml",
]);

/**
 * Classify one changed file. Returns exactly one of:
 *   { kind: "escape-hatch", id, reason }
 *   { kind: "crate", name }
 *   { kind: "ignored" }               — known non-Rust path, no crate involved
 *   { kind: "unrecognized" }          — over-select: force the full workspace
 *
 * @param {ReturnType<typeof buildWorkspaceIndex>} index
 * @param {string} relPath
 */
export function classifyChangedFile(index, relPath) {
  const hatch = matchEscapeHatch(relPath, index);
  if (hatch) return { kind: "escape-hatch", id: hatch.id, reason: hatch.reason };

  const crate = mapPathToCrate(index, relPath);
  if (crate) return { kind: "crate", name: crate.name };

  const topLevel = relPath.split("/")[0];
  if (KNOWN_NON_RUST_TOP_LEVEL.has(topLevel)) return { kind: "ignored" };

  return { kind: "unrecognized" };
}
