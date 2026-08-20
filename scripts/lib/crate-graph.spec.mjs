#!/usr/bin/env node

// Tests for crate-graph.mjs. Run: node --test scripts/lib/crate-graph.spec.mjs
//
// All tests run against small SYNTHETIC `cargo metadata`-shaped fixtures, not
// the real ~40-crate workspace graph — deterministic and fast.

import assert from "node:assert/strict";
import test from "node:test";

import {
  buildWorkspaceIndex,
  buildReverseDependencyGraph,
  transitiveDependents,
  mapPathToCrate,
  matchEscapeHatch,
  classifyChangedFile,
  ESCAPE_HATCH_RULES,
} from "./crate-graph.mjs";

const ROOT = "/repo";

/**
 * Build a synthetic `cargo metadata --format-version=1`-shaped document.
 *
 * @param {Array<{name: string, dir: string, deps?: Array<{name: string, kind?: string}>, procMacro?: boolean}>} pkgs
 */
function fixtureMetadata(pkgs) {
  const idOf = (name) => `path+file://${ROOT}/${pkgs.find((p) => p.name === name).dir}#0.0.0`;
  return {
    workspace_root: ROOT,
    workspace_members: pkgs.map((p) => idOf(p.name)),
    packages: pkgs.map((p) => ({
      id: idOf(p.name),
      name: p.name,
      manifest_path: `${ROOT}/${p.dir}/Cargo.toml`,
      targets: [{ kind: p.procMacro ? ["proc-macro"] : ["lib"] }],
    })),
    resolve: {
      nodes: pkgs.map((p) => ({
        id: idOf(p.name),
        deps: (p.deps ?? []).map((d) => ({
          name: d.name,
          pkg: idOf(d.name),
          dep_kinds: [{ kind: d.kind ?? null, target: null }],
        })),
      })),
    },
  };
}

// Graph: a <- b (normal dep) <- c (normal dep on b); d has a DEV-only dep on
// a (models "d's test suite exercises a, though d's lib never links a"); e is
// isolated. Plus a sibling-prefix-collision pair: "core" and "core_derive"
// (a proc-macro), and "core_ext" which depends on "core_derive".
const PKGS = [
  { name: "a", dir: "crates/a" },
  { name: "b", dir: "crates/b", deps: [{ name: "a" }] },
  { name: "c", dir: "crates/c", deps: [{ name: "b" }] },
  { name: "d", dir: "crates/d", deps: [{ name: "a", kind: "dev" }] },
  { name: "e", dir: "crates/e" },
  { name: "core", dir: "crates/core" },
  { name: "core_derive", dir: "crates/core_derive", procMacro: true },
  { name: "core_ext", dir: "crates/core_ext", deps: [{ name: "core_derive" }] },
];

function buildFixture() {
  const metadata = fixtureMetadata(PKGS);
  const index = buildWorkspaceIndex(metadata);
  const reverse = buildReverseDependencyGraph(metadata, index);
  return { metadata, index, reverse };
}

test("buildWorkspaceIndex maps names to manifestDir and proc-macro flag", () => {
  const { index } = buildFixture();
  assert.equal(index.byName.get("a").manifestDir, "crates/a");
  assert.equal(index.byName.get("a").isProcMacro, false);
  assert.equal(index.byName.get("core_derive").isProcMacro, true);
  assert.equal(index.packages.size, PKGS.length);
});

test("buildReverseDependencyGraph includes normal, build, and dev edges", () => {
  const { reverse } = buildFixture();
  assert.deepEqual([...reverse.get("a")].sort(), ["b", "d"]); // b: normal, d: dev
  assert.deepEqual([...reverse.get("b")].sort(), ["c"]);
  assert.deepEqual([...reverse.get("c")].sort(), []);
  assert.deepEqual([...reverse.get("e")].sort(), []);
});

test("transitiveDependents BFS reaches multi-hop dependents and includes the start set", () => {
  const { reverse } = buildFixture();
  const closure = transitiveDependents(reverse, ["a"]);
  // a's direct dependents are b (normal) and d (dev); c depends on b transitively.
  assert.deepEqual([...closure].sort(), ["a", "b", "c", "d"]);
});

test("transitiveDependents on an isolated crate returns just itself", () => {
  const { reverse } = buildFixture();
  assert.deepEqual([...transitiveDependents(reverse, ["e"])], ["e"]);
});

test("mapPathToCrate matches by path-segment prefix, not naive string prefix", () => {
  const { index } = buildFixture();
  // "core" is NOT a naive-string-prefix false-positive match for files under
  // the sibling "core_derive" directory.
  assert.equal(mapPathToCrate(index, "crates/core_derive/src/lib.rs").name, "core_derive");
  assert.equal(mapPathToCrate(index, "crates/core/src/lib.rs").name, "core");
  assert.equal(mapPathToCrate(index, "crates/core_ext/src/lib.rs").name, "core_ext");
});

test("mapPathToCrate returns null for a path outside every crate directory", () => {
  const { index } = buildFixture();
  assert.equal(mapPathToCrate(index, "docs/readme.md"), null);
  assert.equal(mapPathToCrate(index, "crates/nonexistent/src/lib.rs"), null);
});

test("mapPathToCrate matches the manifest directory itself, not just files under it", () => {
  const { index } = buildFixture();
  assert.equal(mapPathToCrate(index, "crates/a").name, "a");
});

// --- Escape hatches: one discriminating positive + negative case each. ---

test("escape hatch: workspace-manifest fires on root Cargo.toml/.lock, not a crate's own manifest", () => {
  const { index } = buildFixture();
  assert.equal(matchEscapeHatch("Cargo.toml", index).id, "workspace-manifest");
  assert.equal(matchEscapeHatch("Cargo.lock", index).id, "workspace-manifest");
  assert.equal(matchEscapeHatch("crates/a/Cargo.toml", index), null);
});

test("escape hatch: nextest-config fires only on the exact .config/nextest.toml path", () => {
  const { index } = buildFixture();
  assert.equal(matchEscapeHatch(".config/nextest.toml", index).id, "nextest-config");
  assert.equal(matchEscapeHatch(".config/other.toml", index), null);
});

test("escape hatch: scripts-tooling fires on anything under scripts/, not a lookalike top-level dir", () => {
  const { index } = buildFixture();
  assert.equal(matchEscapeHatch("scripts/affected-tests.mjs", index).id, "scripts-tooling");
  assert.equal(matchEscapeHatch("scripts", index).id, "scripts-tooling");
  assert.equal(matchEscapeHatch("script-utils/foo.mjs", index), null);
});

test("escape hatch: verter-identity fires only inside crates/verter_identity, not a sibling", () => {
  const { index } = buildFixture();
  assert.equal(matchEscapeHatch("crates/verter_identity/src/lib.rs", index).id, "verter-identity");
  assert.equal(matchEscapeHatch("crates/verter_identity_other/src/lib.rs", index), null);
});

test("escape hatch: generated-bindings fires on the named generated files and the proto tree", () => {
  const { index } = buildFixture();
  assert.equal(
    matchEscapeHatch("packages/types/audit.generated.ts", index).id,
    "generated-bindings",
  );
  assert.equal(
    matchEscapeHatch("crates/verter_protocol/proto/verter/v1/typeinfo.proto", index).id,
    "generated-bindings",
  );
  assert.equal(matchEscapeHatch("packages/types/other.ts", index), null);
});

test("escape hatch: proc-macro-crate fires for a file owned by a proc-macro crate, not a normal crate", () => {
  const { index } = buildFixture();
  assert.equal(matchEscapeHatch("crates/core_derive/src/lib.rs", index).id, "proc-macro-crate");
  assert.equal(matchEscapeHatch("crates/core/src/lib.rs", index), null);
});

test("escape hatch: ci-workflows fires only under .github/workflows/, not other .github paths", () => {
  const { index } = buildFixture();
  assert.equal(matchEscapeHatch(".github/workflows/ci.yml", index).id, "ci-workflows");
  assert.equal(matchEscapeHatch(".github/ISSUE_TEMPLATE/bug.md", index), null);
});

test("every escape hatch rule id is unique", () => {
  const ids = ESCAPE_HATCH_RULES.map((r) => r.id);
  assert.deepEqual(ids, [...new Set(ids)]);
});

// --- classifyChangedFile end-to-end classification ---

test("classifyChangedFile: crate-owned path classifies as 'crate'", () => {
  const { index } = buildFixture();
  assert.deepEqual(classifyChangedFile(index, "crates/b/src/lib.rs"), { kind: "crate", name: "b" });
});

test("classifyChangedFile: known non-Rust top-level path classifies as 'ignored'", () => {
  const { index } = buildFixture();
  assert.deepEqual(classifyChangedFile(index, "docs/guide.md"), { kind: "ignored" });
  assert.deepEqual(classifyChangedFile(index, ".github/dependabot.yml"), { kind: "ignored" });
});

test("classifyChangedFile: unrecognized top-level path classifies as 'unrecognized' (over-select)", () => {
  const { index } = buildFixture();
  assert.deepEqual(classifyChangedFile(index, "some-new-top-level-dir/file.txt"), {
    kind: "unrecognized",
  });
});

test("classifyChangedFile: escape hatch takes priority over crate mapping", () => {
  const { index } = buildFixture();
  // core_derive is both a matched crate directory AND a proc-macro escape hatch;
  // the escape hatch must win.
  const result = classifyChangedFile(index, "crates/core_derive/src/lib.rs");
  assert.equal(result.kind, "escape-hatch");
  assert.equal(result.id, "proc-macro-crate");
});
