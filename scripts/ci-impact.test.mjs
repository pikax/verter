#!/usr/bin/env node

// Tests for ci-impact.mjs. Run: node --test scripts/ci-impact.test.mjs
//
// The classifier decides which CONSUMER lanes of ci.yml (artifact builds and
// the suites that consume them) a change can affect, from the real Cargo
// dependency graph instead of hand-written `crates/**` wildcards. These tests
// run against small synthetic `cargo metadata`-shaped fixtures. The few tests
// that read the repository check only inventories: that every lane root is a
// crate that exists, and that the lanes whose selection is derived from an
// inventory (provider selectors, compile-contract owners) cover all of it.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  CI_INERT_PATHS,
  COMPILE_CONTRACT_OWNER_CRATES,
  ESCAPE_HATCHES,
  LANE_GATES,
  LANE_ROOTS,
  PROVIDER_PACKAGES,
  classifyCiImpact,
  composeLaneGates,
  formatGithubOutput,
  isCiInert,
  ownedFilesFromFilterOutputs,
} from "./ci-impact.mjs";
import { buildWorkspaceIndex } from "./lib/crate-graph.mjs";
import { PROVIDER_LIVE_SELECTORS } from "./provider-ci-internals.mjs";

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
// crate nothing depends on; harness dev-depends on compiler; derive is a
// proc-macro crate.
const PKGS = [
  { name: "span", dir: "crates/span" },
  { name: "parser", dir: "crates/parser", deps: [{ name: "span" }] },
  { name: "compiler", dir: "crates/compiler", deps: [{ name: "parser" }] },
  { name: "napi", dir: "crates/napi", deps: [{ name: "compiler" }] },
  { name: "wasm", dir: "crates/wasm", deps: [{ name: "compiler" }] },
  { name: "lsp", dir: "crates/lsp", deps: [{ name: "compiler" }] },
  { name: "tool", dir: "crates/tool" },
  { name: "harness", dir: "crates/harness", deps: [{ name: "compiler", kind: "dev" }] },
  { name: "derive", dir: "crates/derive", procMacro: true },
];

const ROOTS = Object.freeze({
  native: ["napi"],
  wasm: ["wasm"],
  lsp: ["lsp"],
  harness: ["harness"],
});

const metadata = fixtureMetadata(PKGS);
const allOff = { native: false, wasm: false, lsp: false, harness: false };
const allOn = { native: true, wasm: true, lsp: true, harness: true };

test("a change inside a lane root's dependency closure impacts that lane and no other", () => {
  const result = classifyCiImpact(["crates/lsp/src/lib.rs"], metadata, ROOTS);
  assert.equal(result.full, false);
  assert.deepEqual(result.directCrates, ["lsp"]);
  assert.deepEqual(result.lanes, { ...allOff, lsp: true });

  const shared = classifyCiImpact(["crates/span/src/lib.rs"], metadata, ROOTS);
  assert.deepEqual(shared.lanes, allOn);
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
  assert.deepEqual(isolated.lanes, allOff);

  // harness only dev-depends on compiler; its suite still exercises it.
  const dev = classifyCiImpact(["crates/compiler/src/lib.rs"], metadata, ROOTS);
  assert.equal(dev.lanes.harness, true);
});

test("a non-crate file a path filter owns, or an explicitly CI-inert one, impacts nothing", () => {
  const owned = new Set([
    "package.json",
    "scripts/jetbrains-gate.mjs",
    "pnpm-lock.yaml",
    "packages/vue-vscode/src/extension.ts",
  ]);
  const result = classifyCiImpact(
    [
      "package.json",
      "scripts/jetbrains-gate.mjs",
      "pnpm-lock.yaml",
      "packages/vue-vscode/src/extension.ts",
      "docs/guide.md",
      "CHANGELOG.md",
    ],
    metadata,
    ROOTS,
    { ownedFiles: owned },
  );
  assert.equal(result.full, false, JSON.stringify(result.fullReasons));
  assert.deepEqual(result.directCrates, []);
  assert.deepEqual(result.lanes, allOff);
  assert.equal(result.ownedNonRust.length, 6);
});

test("no top-level directory is inert by assumption: an unowned file under one falls back", () => {
  // Both are read by Rust tests today; the rust filter owns them. Without an
  // owner they must force the full graph, never be waved through because
  // their directory looked like a non-input.
  for (const file of [
    "schemas/scanners-replacement-v1.schema.json",
    "test-corpora/style-ir/css-baseline-legacy.json",
    "extensions/lapce/src/lib.rs",
    "examples/reference/app.vue",
    "tools/something.mjs",
    "mcp/config.json",
    ".github/README.md",
  ]) {
    assert.equal(isCiInert(file), false, file);
    const result = classifyCiImpact([file], metadata, ROOTS, { ownedFiles: new Set() });
    assert.equal(result.full, true, file);
    assert.equal(result.fullReasons[0].id, "unrecognized-path", file);
  }
  // The inert list is small and names only files with no CI consumer.
  assert.ok(CI_INERT_PATHS.length <= 16);
  for (const entry of CI_INERT_PATHS) {
    assert.doesNotMatch(
      entry,
      /^(schemas|test-corpora|extensions|examples|tools|mcp|packages|crates|scripts)\b/,
    );
  }
});

test("the rust filter owns the directories Rust tests read", () => {
  const ci = readFileSync(join(REPO_ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const start = ci.indexOf("\n            rust:\n");
  assert.notEqual(start, -1, "ci.yml must declare the rust filter");
  const rest = ci.slice(start + 1);
  const next = rest.search(/\n(?: {12}#[^\n]*\n)* {12}[a-z_]+:\n/);
  const block = next === -1 ? rest : rest.slice(0, next);
  for (const owned of ["schemas/**", "test-corpora/**"]) {
    assert.ok(block.includes(`- '${owned}'`), `the rust filter must own ${owned}`);
  }
});

test("a non-crate file no path filter owns forces every lane on rather than guessing", () => {
  for (const file of ["scripts/brand-new-tool.mjs", "brand-new-dir/thing.rs", "tsconfig.json"]) {
    const result = classifyCiImpact(["crates/tool/src/main.rs", file], metadata, ROOTS, {
      ownedFiles: new Set(["scripts/jetbrains-gate.mjs"]),
    });
    assert.equal(result.full, true, file);
    assert.equal(result.fullReasons[0].id, "unrecognized-path", file);
    assert.equal(result.fullReasons[0].file, file);
    assert.deepEqual(result.lanes, allOn, file);
  }
});

test("the classifier's own escape hatches force every lane on and name why", () => {
  const cases = {
    "Cargo.lock": "workspace-manifest",
    "Cargo.toml": "workspace-manifest",
    ".config/nextest.toml": "nextest-config",
    "rust-toolchain.toml": "toolchain",
    ".cargo/config.toml": "toolchain",
    ".github/workflows/ci.yml": "ci-workflows",
    ".github/actions/download-artifact/action.yml": "ci-actions",
    "scripts/ci-impact.mjs": "lane-classifier",
    "scripts/lib/crate-graph.mjs": "lane-classifier",
    "crates/derive/src/lib.rs": "proc-macro-crate",
  };
  for (const [file, id] of Object.entries(cases)) {
    // Owned by a filter too: a hatch wins over ownership.
    const result = classifyCiImpact(["crates/tool/src/main.rs", file], metadata, ROOTS, {
      ownedFiles: new Set([file]),
    });
    assert.equal(result.full, true, file);
    assert.deepEqual(
      result.fullReasons.map((r) => [r.file, r.id]),
      [[file, id]],
      file,
    );
    assert.deepEqual(result.lanes, allOn, file);
  }
  // Ordinary tooling is NOT a hatch here: its lane's filter owns it, unlike
  // affected-tests.mjs, which has no filters to defer to.
  const index = buildWorkspaceIndex(metadata);
  assert.equal(
    ESCAPE_HATCHES.some((rule) => rule.test("scripts/jetbrains-gate.mjs", index)),
    false,
  );
});

test("a lane root that is not a workspace member is refused, never silently empty", () => {
  assert.throws(
    () => classifyCiImpact(["crates/lsp/src/lib.rs"], metadata, { lsp: ["lsp", "ghost"] }),
    /root "ghost" is not a workspace member/,
  );
});

test("composeLaneGates ORs filters, impact lanes and earlier gates, and fails closed on wiring", () => {
  const gates = {
    rust: { filters: ["rust"] },
    wasm: { filters: ["wasm"], impact: ["wasm"] },
    vscode: { filters: ["vscode"], impact: ["lsp"] },
    contracts: { filters: [], impact: ["lsp"] },
    wasm_artifact: { gates: ["wasm", "vscode"] },
  };
  const filters = { rust: "true", wasm: "false", vscode: "false", wasm_files: "[]" };
  const impact = { full: false, lanes: { wasm: false, lsp: true } };
  assert.deepEqual(composeLaneGates(filters, impact, gates), {
    rust: "true",
    wasm: "false",
    vscode: "true",
    contracts: "true",
    wasm_artifact: "true",
  });

  // The path filter alone is enough.
  assert.equal(composeLaneGates({ ...filters, wasm: "true" }, impact, gates).wasm, "true");
  // A full fallback turns on every impact-bearing gate and leaves pass-through
  // gates to their filter.
  assert.deepEqual(
    composeLaneGates({ ...filters, rust: "false" }, { full: true, lanes: {} }, gates),
    { rust: "false", wasm: "true", vscode: "true", contracts: "true", wasm_artifact: "true" },
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
  // A gate may only depend on gates evaluated before it.
  assert.throws(
    () => composeLaneGates(filters, impact, { a: { gates: ["b"] }, b: { filters: ["rust"] } }),
    /depends on gate "b", which is not evaluated before it/,
  );
});

test("every consumer gate implies its producer's artifact gate, for every input combination", () => {
  // The three consumers of the native/wasm artifacts and the two subsystems,
  // over every combination of the inputs that can switch them.
  const filterNames = ["js", "wasm", "playground", "transport"];
  const impactNames = ["native", "wasm", "playground"];
  const baseFilters = Object.fromEntries(
    Object.values(LANE_GATES)
      .flatMap((spec) => spec.filters ?? [])
      .map((name) => [name, "false"]),
  );
  const baseLanes = Object.fromEntries(Object.keys(LANE_ROOTS).map((lane) => [lane, false]));
  const bits = filterNames.length + impactNames.length;
  let playgroundOnlyChecked = false;
  for (let mask = 0; mask < 1 << bits; mask++) {
    const filters = { ...baseFilters };
    const lanes = { ...baseLanes };
    filterNames.forEach((name, i) => {
      filters[name] = mask & (1 << i) ? "true" : "false";
    });
    impactNames.forEach((name, i) => {
      lanes[name] = Boolean(mask & (1 << (filterNames.length + i)));
    });
    const gates = composeLaneGates(filters, { full: false, lanes });
    const label = JSON.stringify({ filters: filterNames.map((n) => filters[n]), lanes });
    for (const consumer of ["native", "playground", "transport"]) {
      if (gates[consumer] === "true") {
        assert.equal(
          gates.native_artifact,
          "true",
          `${consumer} needs the native artifact ${label}`,
        );
      }
    }
    for (const consumer of ["wasm", "playground", "transport"]) {
      if (gates[consumer] === "true") {
        assert.equal(gates.wasm_artifact, "true", `${consumer} needs the wasm artifact ${label}`);
      }
    }
    // Either subsystem moving demands the equivalence check, and with it both
    // artifacts.
    if (lanes.native || lanes.wasm) {
      assert.equal(gates.transport, "true", `transport must run ${label}`);
    }
    // A playground-only change needs the artifacts but is not a change to the
    // native subsystem: native-test must stay off.
    if (mask === 1 << filterNames.indexOf("playground")) {
      assert.equal(gates.native, "false", label);
      assert.equal(gates.native_artifact, "true", label);
      assert.equal(gates.wasm_artifact, "true", label);
      playgroundOnlyChecked = true;
    }
  }
  assert.ok(playgroundOnlyChecked);
});

test("ownedFilesFromFilterOutputs unions every filter's file list except the catch-all", () => {
  const owned = ownedFilesFromFilterOutputs({
    js: "true",
    js_files: JSON.stringify(["package.json", "packages/a/x.ts"]),
    wasm: "false",
    wasm_files: "[]",
    jetbrains_files: JSON.stringify(["scripts\\jetbrains-gate.mjs"]),
    any_files: JSON.stringify(["everything.md"]),
  });
  assert.deepEqual([...owned].sort(), [
    "package.json",
    "packages/a/x.ts",
    "scripts/jetbrains-gate.mjs",
  ]);
  assert.throws(() => ownedFilesFromFilterOutputs({ x_files: "not json" }), /not a JSON file list/);
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

test("the provider lane's roots cover every package its selectors own tests in", () => {
  const selectorPackages = [...new Set(PROVIDER_LIVE_SELECTORS.map((s) => s.package))].sort();
  assert.deepEqual([...PROVIDER_PACKAGES].sort(), selectorPackages);
  for (const pkg of selectorPackages) {
    assert.ok(LANE_ROOTS.providers.includes(pkg), `providers lane must root ${pkg}`);
  }
  // The relay shim is owned by the tsgo lane but is not a dependency of the
  // LSP crate, so a derived root set is the only thing that keeps it in.
  assert.ok(LANE_ROOTS.providers.includes("verter_relay_shim"));
});

test("the compile-contract lane's roots cover every owner the runner lists", () => {
  const listed = spawnSync(
    process.execPath,
    [join(REPO_ROOT, "scripts", "compile-contracts.mjs"), "--list-owners"],
    { encoding: "utf8", cwd: REPO_ROOT },
  );
  assert.equal(listed.status, 0, listed.stderr);
  const owners = listed.stdout.split("\n").filter(Boolean).sort();
  assert.deepEqual(Object.keys(COMPILE_CONTRACT_OWNER_CRATES).sort(), owners);
  for (const crate of Object.values(COMPILE_CONTRACT_OWNER_CRATES)) {
    assert.ok(
      LANE_ROOTS.compiler_contracts.includes(crate),
      `compile contracts must root ${crate}`,
    );
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
