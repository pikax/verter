#!/usr/bin/env node

// Tests for ci-impact.mjs. Run: node --test scripts/ci-impact.test.mjs
//
// The classifier decides which CONSUMER lanes of ci.yml (artifact builds and
// the suites that consume them) a change can affect, from the real Cargo
// dependency graph instead of hand-written `crates/**` wildcards. These tests
// run against small synthetic `cargo metadata`-shaped fixtures; the one
// workspace-reading test only checks that every declared lane root names a
// crate that exists, so a renamed crate cannot leave a lane silently rootless.

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  LANE_GATES,
  LANE_ROOTS,
  classifyCiImpact,
  composeLaneGates,
  formatGithubOutput,
} from "./ci-impact.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const ROOT = "/repo";

/** Synthetic `cargo metadata --format-version=1` document. */
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

// span <- parser <- compiler <- {napi, wasm, lsp}; tool is a standalone
// crate nothing depends on; harness dev-depends on compiler.
const PKGS = [
  { name: "span", dir: "crates/span" },
  { name: "parser", dir: "crates/parser", deps: [{ name: "span" }] },
  { name: "compiler", dir: "crates/compiler", deps: [{ name: "parser" }] },
  { name: "napi", dir: "crates/napi", deps: [{ name: "compiler" }] },
  { name: "wasm", dir: "crates/wasm", deps: [{ name: "compiler" }] },
  { name: "lsp", dir: "crates/lsp", deps: [{ name: "compiler" }] },
  { name: "tool", dir: "crates/tool" },
  { name: "harness", dir: "crates/harness", deps: [{ name: "compiler", kind: "dev" }] },
];

const ROOTS = Object.freeze({
  native: ["napi"],
  wasm: ["wasm"],
  lsp: ["lsp"],
  harness: ["harness"],
});

const metadata = fixtureMetadata(PKGS);

test("a change inside a lane root's dependency closure impacts that lane and no other", () => {
  const result = classifyCiImpact(["crates/lsp/src/lib.rs"], metadata, ROOTS);
  assert.equal(result.full, false);
  assert.deepEqual(result.directCrates, ["lsp"]);
  assert.deepEqual(result.lanes, { native: false, wasm: false, lsp: true, harness: false });

  const shared = classifyCiImpact(["crates/span/src/lib.rs"], metadata, ROOTS);
  assert.deepEqual(shared.lanes, { native: true, wasm: true, lsp: true, harness: true });
  assert.deepEqual(shared.impactedCrates, [
    "compiler",
    "harness",
    "lsp",
    "napi",
    "parser",
    "span",
    "wasm",
  ]);
});

test("a crate nothing depends on impacts no lane, and a dev-dependency edge counts", () => {
  const isolated = classifyCiImpact(["crates/tool/src/main.rs"], metadata, ROOTS);
  assert.equal(isolated.full, false);
  assert.deepEqual(isolated.lanes, { native: false, wasm: false, lsp: false, harness: false });

  // harness only dev-depends on compiler; its suite still exercises it.
  const dev = classifyCiImpact(["crates/compiler/src/lib.rs"], metadata, ROOTS);
  assert.equal(dev.lanes.harness, true);
});

test("non-Rust paths impact nothing and force nothing", () => {
  const result = classifyCiImpact(
    ["docs/guide.md", "packages/vue-vscode/src/extension.ts", ".github/README.md"],
    metadata,
    ROOTS,
  );
  assert.equal(result.full, false);
  assert.deepEqual(result.directCrates, []);
  assert.deepEqual(Object.values(result.lanes), [false, false, false, false]);
});

test("an escape hatch forces every lane on and names why", () => {
  for (const file of [
    "Cargo.lock",
    ".config/nextest.toml",
    "scripts/ci-impact.mjs",
    ".github/workflows/ci.yml",
    ".github/actions/download-artifact/action.yml",
    "rust-toolchain.toml",
    ".cargo/config.toml",
  ]) {
    const result = classifyCiImpact(["crates/tool/src/main.rs", file], metadata, ROOTS);
    assert.equal(result.full, true, file);
    assert.ok(
      result.fullReasons.some((r) => r.file === file),
      `${file} must be the reason`,
    );
    assert.deepEqual(Object.values(result.lanes), [true, true, true, true], file);
  }
});

test("an unrecognized top-level path forces every lane on rather than guessing", () => {
  const result = classifyCiImpact(["brand-new-dir/thing.rs"], metadata, ROOTS);
  assert.equal(result.full, true);
  assert.equal(result.fullReasons[0].id, "unrecognized-path");
});

test("a lane root that is not a workspace member is refused, never silently empty", () => {
  assert.throws(
    () => classifyCiImpact(["crates/lsp/src/lib.rs"], metadata, { lsp: ["lsp", "ghost"] }),
    /root "ghost" is not a workspace member/,
  );
});

test("composeLaneGates ORs the path filter with the impact and fails closed on a missing filter", () => {
  const gates = {
    rust: { filters: ["rust"] },
    wasm: { filters: ["wasm"], impact: ["wasm"] },
    vscode: { filters: ["vscode"], impact: ["lsp"] },
    contracts: { filters: [], impact: ["lsp"] },
  };
  const filters = { rust: "true", wasm: "false", vscode: "false", wasm_files: "[]" };
  const impact = { full: false, lanes: { wasm: false, lsp: true } };
  assert.deepEqual(composeLaneGates(filters, impact, gates), {
    rust: "true",
    wasm: "false",
    vscode: "true",
    contracts: "true",
  });

  // The path filter alone is enough.
  assert.equal(composeLaneGates({ ...filters, wasm: "true" }, impact, gates).wasm, "true");
  // A full fallback turns on every impact-bearing gate and leaves pass-through
  // gates to their filter.
  assert.deepEqual(
    composeLaneGates({ ...filters, rust: "false" }, { full: true, lanes: {} }, gates),
    { rust: "false", wasm: "true", vscode: "true", contracts: "true" },
  );
  // A gate naming a filter the workflow did not produce is a wiring bug.
  assert.throws(
    () => composeLaneGates({ rust: "true" }, impact, gates),
    /filter "wasm" is not among the paths-filter outputs/,
  );
  // Unknown impact lane names are refused the same way.
  assert.throws(
    () => composeLaneGates(filters, impact, { x: { filters: [], impact: ["nope"] } }),
    /impact lane "nope"/,
  );
});

test("formatGithubOutput emits one gate line per gate plus the fallback flag", () => {
  const lines = formatGithubOutput({ rust: "true", wasm: "false" }, { full: true });
  assert.deepEqual(lines, ["gate_rust=true", "gate_wasm=false", "impact_full=true"]);
});

test("every gate's impact lanes are declared roots, and every root lane is used by a gate", () => {
  const used = new Set();
  for (const [gate, spec] of Object.entries(LANE_GATES)) {
    for (const lane of spec.impact ?? []) {
      assert.ok(lane in LANE_ROOTS, `gate ${gate} names undeclared impact lane ${lane}`);
      used.add(lane);
    }
  }
  for (const lane of Object.keys(LANE_ROOTS)) {
    assert.ok(used.has(lane), `impact lane ${lane} is declared but no gate consumes it`);
  }
});

test("every declared lane root is a crate in this workspace", () => {
  const names = new Set();
  const cratesDir = join(REPO_ROOT, "crates");
  for (const entry of readdirSync(cratesDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    let manifest;
    try {
      manifest = readFileSync(join(cratesDir, entry.name, "Cargo.toml"), "utf8");
    } catch {
      continue;
    }
    const name = manifest.match(/^\s*name\s*=\s*"([^"]+)"/m)?.[1];
    if (name) names.add(name);
  }
  for (const [lane, roots] of Object.entries(LANE_ROOTS)) {
    for (const root of roots) {
      assert.ok(names.has(root), `lane ${lane} root ${root} is not a crate under crates/`);
    }
  }
});
