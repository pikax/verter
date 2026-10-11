#!/usr/bin/env node

// Tests for ci-impact.mjs. Run: node --test scripts/ci-impact.test.mjs
//
// The classifier is the one lane-selection authority for ci.yml: its path
// filters own every non-crate input, and the real Cargo dependency graph
// decides which CONSUMER lanes (artifact builds and the suites that consume
// them) a crate change can affect. Most tests run against small synthetic
// `cargo metadata`-shaped fixtures. The tests that read the repository check
// inventories: that every lane root is a crate that exists, that the lanes
// whose selection is derived from an inventory (provider selectors,
// compile-contract owners) cover all of it, that every tracked file is owned
// or explicitly inert, and that no filter pattern matches nothing.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  CI_INERT_PATHS,
  COMPILE_CONTRACT_OWNER_CRATES,
  ESCAPE_HATCHES,
  LANE_GATES,
  LANE_ROOTS,
  PATH_FILTERS,
  PROVIDER_PACKAGES,
  auditSelection,
  classifyCiImpact,
  compileGlob,
  compilePathFilters,
  composeLaneGates,
  formatGithubOutput,
  isCiInert,
  matchPathFilters,
  releaseGates,
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

// The real lanes: every root crate as an independent fixture member, so a
// changed crate impacts exactly the lanes that root it.
const LANE_ROOT_CRATES = [...new Set(Object.values(LANE_ROOTS).flat())];
const laneMetadata = fixtureMetadata(
  LANE_ROOT_CRATES.map((name) => ({ name, dir: `crates/${name}` })),
);

/** The gates ci.yml would publish for `files`, through the real filters. */
function select(files) {
  const { hits, owned } = matchPathFilters(files);
  const impact = classifyCiImpact(files, laneMetadata, LANE_ROOTS, { ownedFiles: owned });
  return { hits, impact, gates: composeLaneGates(hits, impact) };
}

const gatesOn = (gates) =>
  Object.keys(gates)
    .filter((gate) => gates[gate] === "true")
    .sort();

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
      "CONTRIBUTING.md",
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
  // The inert list names what no ci.yml job reads, never a whole input tree:
  // a new file under one must reach an owner or the fallback, not silence.
  for (const entry of CI_INERT_PATHS) {
    assert.doesNotMatch(
      entry,
      /^(?:\*\*|(?:schemas|test-corpora|extensions|editors|examples|tools|mcp|packages|crates|scripts|tests|\.github)\/\*\*)$/,
      entry,
    );
  }
});

test("the rust filter owns the files Rust tests read, and only those", () => {
  for (const file of [
    "schemas/scanners-replacement-v1.schema.json",
    "test-corpora/style-ir/css-baseline-legacy.json",
    // include_str!-ed by verter_compiler and verter_session tests
    "tests/sfc-projection/STP12/fixtures/jsx.vue",
    "tests/sfc-projection/STP16/probes/components/Picker.vue.ts",
    // read by the STS0 profile gate and the leaked-path scan
    "tests/sfc-projection/STS0/products/svelte-projection-policy.json",
    "tests/sfc-projection/STP6/packed/types/index.d.ts.map",
    "docs/audit-footprint/api-reference.md",
    "CHANGELOG.md",
    ".gitignore",
    "clippy.toml",
  ]) {
    assert.equal(matchPathFilters([file]).hits.rust, true, `the rust filter must own ${file}`);
  }
  // The rest of the node tree is the verifiers' input, not the Rust suite's.
  for (const file of [
    "tests/sfc-projection/STP16/manifest.json",
    "tests/sfc-projection/STP9/protocol.mjs",
    "docs/guide/getting-started.md",
  ]) {
    assert.equal(matchPathFilters([file]).hits.rust, false, file);
  }
});

test("a non-crate file no path filter owns forces every lane on rather than guessing", () => {
  for (const file of ["scripts/brand-new-tool.mjs", "brand-new-dir/thing.rs", "tsconfig.json"]) {
    const result = classifyCiImpact(["crates/tool/src/main.rs", file], metadata, ROOTS, {
      ownedFiles: new Set(["scripts/jetbrains-gate.mjs"]),
    });
    assert.equal(result.full, true, file);
    assert.equal(result.everything, true, file);
    assert.equal(result.fullReasons[0].id, "unrecognized-path", file);
    assert.equal(result.fullReasons[0].file, file);
    assert.deepEqual(result.lanes, allOn, file);
  }
});

test("the classifier's own escape hatches force every lane on and name why", () => {
  // [id, whether every gate runs rather than every impact-bearing one]
  const cases = {
    "Cargo.lock": ["workspace-manifest", false],
    "Cargo.toml": ["workspace-manifest", false],
    ".config/nextest.toml": ["nextest-config", false],
    "rust-toolchain.toml": ["toolchain", false],
    ".cargo/config.toml": ["toolchain", false],
    ".github/workflows/ci.yml": ["ci-workflow", true],
    ".github/actions/download-artifact/action.yml": ["ci-actions", true],
    "scripts/ci-impact.mjs": ["lane-classifier", true],
    "scripts/lib/crate-graph.mjs": ["lane-classifier", true],
    "crates/derive/src/lib.rs": ["proc-macro-crate", false],
  };
  for (const [file, [id, everything]] of Object.entries(cases)) {
    // Owned by a filter too: a hatch wins over ownership.
    const result = classifyCiImpact(["crates/tool/src/main.rs", file], metadata, ROOTS, {
      ownedFiles: new Set([file]),
    });
    assert.equal(result.full, true, file);
    assert.equal(result.everything, everything, file);
    assert.deepEqual(
      result.fullReasons.map((r) => [r.file, r.id]),
      [[file, id]],
      file,
    );
    assert.deepEqual(result.lanes, allOn, file);
  }
  // Ordinary tooling is NOT a hatch here: its lane's filter owns it, unlike
  // affected-tests.mjs, which has no filters to defer to. Neither is a
  // workflow other than ci.yml: it runs on its own trigger.
  const index = buildWorkspaceIndex(metadata);
  for (const file of ["scripts/jetbrains-gate.mjs", ".github/workflows/nightly.yml"]) {
    assert.equal(
      ESCAPE_HATCHES.some((rule) => rule.test(file, index)),
      false,
      file,
    );
  }
});

test("a change to CI-inert paths only runs no lane", () => {
  // The shape of a kernel architecture pull request: prose and the inventory
  // products it ratifies, which no CI job reads.
  const { hits, impact, gates } = select([
    "docs/arch/kernel/carrier-frontend-backend.md",
    "tests/kernel/CPF0/products/carrier-split-inventory.v1.json",
    "CONTRIBUTING.md",
    "tests/web-product/WDX0/manifest.json",
    "examples/src/App.vue",
    ".github/workflows/benchmark.yml",
  ]);
  assert.equal(impact.full, false, JSON.stringify(impact.fullReasons));
  assert.deepEqual(
    Object.keys(hits).filter((name) => hits[name]),
    [],
  );
  assert.deepEqual(gatesOn(gates), []);
});

test("an exclusion removes only what the filter's own includes matched", () => {
  // The js filter covers packages/** except the playground, which has its
  // own lane; a paths-filter negation once made it cover every other file.
  const playground = select(["packages/playground/src/App.vue"]);
  assert.equal(playground.hits.js, false);
  assert.deepEqual(gatesOn(playground.gates), ["native_artifact", "playground", "wasm_artifact"]);
  assert.equal(matchPathFilters(["packages/vue-vscode/src/extension.ts"]).hits.js, true);
  assert.equal(matchPathFilters(["docs/guide.md"]).hits.js, false);

  const { hits } = matchPathFilters(
    ["a/x.ts", "a/skip/y.ts"],
    compilePathFilters({ lane: { include: ["a/**"], exclude: ["a/skip/**"] } }),
  );
  assert.deepEqual(hits, { lane: true });
  assert.deepEqual(
    matchPathFilters(["a/skip/y.ts", "b/z.ts"], {
      lane: { include: ["a/**"], exclude: ["a/skip/**"] },
    }).hits,
    { lane: false },
  );
});

test("filter globs are plain positive globs; anything else is refused", () => {
  const cases = [
    ["crates/**", ["crates/a/src/lib.rs", "crates/.cargo-ok"], ["crate/a.rs", "crates"]],
    [
      "scripts/gate*.mjs",
      ["scripts/gate.mjs", "scripts/gate-internals.mjs"],
      ["scripts/lib/gate.mjs"],
    ],
    ["tsconfig*.json", ["tsconfig.json", "tsconfig.base.json"], ["packages/a/tsconfig.json"]],
    ["a/**/b.md", ["a/b.md", "a/x/b.md", "a/x/y/b.md"], ["a/xb.md", "b.md"]],
    ["**", ["anything", ".hidden/file"], []],
    [".cargo/**", [".cargo/config.toml"], ["cargo/config.toml"]],
  ];
  for (const [glob, yes, no] of cases) {
    const re = compileGlob(glob);
    for (const file of yes) assert.ok(re.test(file), `${glob} must match ${file}`);
    for (const file of no) assert.ok(!re.test(file), `${glob} must not match ${file}`);
  }
  for (const glob of [
    "!packages/playground/**",
    "packages/{a,b}/**",
    "packages/[ab]/**",
    "file?.md",
    "a/**b",
    "/abs/**",
    "./rel/**",
    "dir/",
    "a//b",
    "a\\b",
    "",
  ]) {
    assert.throws(() => compileGlob(glob), /ci-impact: /, glob);
  }
  assert.throws(() => compilePathFilters({ lane: [] }), /must be a non-empty glob list/);
  assert.throws(
    () => compilePathFilters({ lane: { include: ["a/**"], exclude: "a/b/**" } }),
    /must be a non-empty glob list/,
  );
});

test("the sfc-projection verifiers run for their node trees and crates, not for JavaScript changes", () => {
  for (const file of [
    "tests/sfc-projection/STP16/probes/components/Picker.vue.ts",
    "scripts/sfc-projection/verify-node.mjs",
    "packages/framework-conformance-harness/evidence/svelte-options.tsv",
    "crates/verter_compiler/src/lib.rs",
    "crates/verter_validation_probe/manifest/svelte.toml",
  ]) {
    assert.equal(select([file]).gates.sfc_projection, "true", file);
  }
  for (const file of ["packages/vue-vscode/src/extension.ts", "scripts/perf-breakdown.mjs"]) {
    assert.equal(select([file]).gates.sfc_projection, "false", file);
  }
});

test("a script with no lane consumer runs the scripts' self-tests and nothing else", () => {
  assert.deepEqual(gatesOn(select(["scripts/perf-breakdown.mjs"]).gates), ["js"]);
});

test("ci.yml, the local actions, the classifier and an unclassified path run every lane", () => {
  for (const file of [
    ".github/workflows/ci.yml",
    ".github/actions/download-artifact/action.yml",
    "scripts/ci-impact.mjs",
    "brand-new-dir/thing.txt",
  ]) {
    const { impact, gates } = select([file]);
    assert.equal(impact.everything, true, file);
    assert.deepEqual(gatesOn(gates), Object.keys(LANE_GATES).sort(), file);
  }
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
  const filters = { rust: true, wasm: false, vscode: false };
  const impact = { full: false, lanes: { wasm: false, lsp: true } };
  assert.deepEqual(composeLaneGates(filters, impact, gates), {
    rust: "true",
    wasm: "false",
    vscode: "true",
    contracts: "true",
    wasm_artifact: "true",
  });

  // The path filter alone is enough.
  assert.equal(composeLaneGates({ ...filters, wasm: true }, impact, gates).wasm, "true");
  // A full fallback turns on every impact-bearing gate and leaves pass-through
  // gates to their filter.
  assert.deepEqual(
    composeLaneGates({ ...filters, rust: false }, { full: true, lanes: {} }, gates),
    { rust: "false", wasm: "true", vscode: "true", contracts: "true", wasm_artifact: "true" },
  );
  // Everything turns pass-through gates on as well.
  assert.deepEqual(
    composeLaneGates(
      { ...filters, rust: false },
      { full: true, everything: true, lanes: {} },
      gates,
    ),
    { rust: "true", wasm: "true", vscode: "true", contracts: "true", wasm_artifact: "true" },
  );
  // A gate naming a filter that does not exist is a wiring bug.
  assert.throws(
    () => composeLaneGates({ rust: true }, impact, gates),
    /names filter "wasm", which is not a path filter/,
  );
  // Every gate's filters are real path filters.
  for (const [gate, spec] of Object.entries(LANE_GATES)) {
    for (const filter of spec.filters ?? []) {
      assert.ok(filter in PATH_FILTERS, `gate ${gate} names unknown filter ${filter}`);
    }
  }
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
  const baseFilters = Object.fromEntries(Object.keys(PATH_FILTERS).map((name) => [name, false]));
  const baseLanes = Object.fromEntries(Object.keys(LANE_ROOTS).map((lane) => [lane, false]));
  const bits = filterNames.length + impactNames.length;
  let playgroundOnlyChecked = false;
  for (let mask = 0; mask < 1 << bits; mask++) {
    const filters = { ...baseFilters };
    const lanes = { ...baseLanes };
    filterNames.forEach((name, i) => {
      filters[name] = Boolean(mask & (1 << i));
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

test("matchPathFilters reports every filter and owns exactly the files some filter matched", () => {
  const { hits, owned } = matchPathFilters([
    "package.json",
    "packages/vue-vscode/src/extension.ts",
    "scripts/jetbrains-gate.mjs",
    "docs/guide.md",
  ]);
  assert.deepEqual(Object.keys(hits).sort(), Object.keys(PATH_FILTERS).sort());
  assert.equal(hits.jetbrains, true);
  assert.equal(hits.js, true);
  assert.equal(hits.proto, true);
  assert.deepEqual([...owned].sort(), [
    "package.json",
    "packages/vue-vscode/src/extension.ts",
    "scripts/jetbrains-gate.mjs",
  ]);
});

test("formatGithubOutput emits one gate line per gate plus the fallback flag", () => {
  const lines = formatGithubOutput({ rust: "true", wasm: "false" }, { full: true });
  assert.deepEqual(lines, ["gate_rust=true", "gate_wasm=false", "impact_full=true", "release="]);
});

// A release pull request's tests are CI's own lanes, selected as for any PR; the
// tag's release reuses them. Its squash commit on main was already tested as
// that pull request's head, so CI does not test the same tree again.
test("a release pull request keeps its lanes and names its kind; its landed commit runs none", () => {
  const gates = { rust: "true", wasm: "true", js: "false" };
  assert.equal(releaseGates(gates, ""), gates);
  assert.equal(releaseGates(gates, "project"), gates);
  assert.equal(releaseGates(gates, "ide"), gates);
  assert.deepEqual(releaseGates(gates, "landed"), { rust: "false", wasm: "false", js: "false" });
  assert.deepEqual(formatGithubOutput(releaseGates(gates, "ide"), { full: true }, "ide"), [
    "gate_rust=true",
    "gate_wasm=true",
    "gate_js=false",
    "impact_full=true",
    "release=ide",
  ]);
  assert.deepEqual(formatGithubOutput(releaseGates(gates, "landed"), { full: true }, "landed"), [
    "gate_rust=false",
    "gate_wasm=false",
    "gate_js=false",
    "impact_full=false",
    "release=landed",
  ]);

  const dir = mkdtempSync(join(tmpdir(), "ci-impact-release-"));
  try {
    const changedFilesPath = join(dir, "changed-files.json");
    const metadataPath = join(dir, "metadata.json");
    const githubOutput = join(dir, "github-output");
    // A change that selects every lane, so dropping one shows.
    writeFileSync(changedFilesPath, JSON.stringify([".github/workflows/ci.yml", "Cargo.toml"]));
    writeFileSync(metadataPath, JSON.stringify(laneMetadata));
    const run = (release) => {
      writeFileSync(githubOutput, "");
      const result = spawnSync(
        process.execPath,
        [
          join(SCRIPT_DIR, "ci-impact.mjs"),
          "--changed-files",
          changedFilesPath,
          "--metadata",
          metadataPath,
          "--github-output",
          githubOutput,
          "--release",
          release,
        ],
        { encoding: "utf8", cwd: REPO_ROOT, env: { ...process.env, CI_IMPACT_RELEASE: "" } },
      );
      const lines = readFileSync(githubOutput, "utf8").trimEnd().split(/\r?\n/u);
      return { result, lines, gateLines: lines.filter((line) => line.startsWith("gate_")) };
    };
    const pr = run("project");
    assert.equal(pr.result.status, 0, pr.result.stderr);
    assert.ok(
      pr.gateLines.length > 0 && pr.gateLines.every((line) => line.endsWith("=true")),
      pr.lines.join(" "),
    );
    assert.ok(pr.lines.includes("release=project"), pr.lines.join(" "));
    assert.match(
      pr.result.stdout,
      /Release pull request \(project\): the release rehearsal runs beside these lanes/u,
    );
    const landed = run("landed");
    assert.equal(landed.result.status, 0, landed.result.stderr);
    assert.ok(
      landed.gateLines.every((line) => line.endsWith("=false")),
      landed.lines.join(" "),
    );
    assert.ok(
      landed.lines.includes("impact_full=false") && landed.lines.includes("release=landed"),
      landed.lines.join(" "),
    );
    assert.equal(run("beta").result.status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
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

test("the CLI reads an arbitrarily large changed-file list from a file, not the environment", () => {
  // The CI step hands the classifier the changed-file list through a file:
  // on a large diff it exceeds Linux's 128 KiB cap on one environment string
  // and bash cannot even be started.
  const ownedTs = Array.from(
    { length: 6000 },
    (_, i) => `packages/some-long-package-name/src/generated/module-${i}.ts`,
  );
  const changedFiles = ["crates/verter_wasm/src/lib.rs", ...ownedTs];

  const dir = mkdtempSync(join(tmpdir(), "ci-impact-"));
  try {
    const changedFilesPath = join(dir, "changed-files.json");
    const metadataPath = join(dir, "metadata.json");
    const githubOutput = join(dir, "github-output");
    writeFileSync(changedFilesPath, JSON.stringify(changedFiles));
    writeFileSync(metadataPath, JSON.stringify(laneMetadata));
    assert.ok(statSync(changedFilesPath).size > 256 * 1024, "the fixture must exceed the env cap");

    const env = { ...process.env };
    delete env.CI_IMPACT_CHANGED_FILES_JSON;
    const run = spawnSync(
      process.execPath,
      [
        join(SCRIPT_DIR, "ci-impact.mjs"),
        "--changed-files",
        changedFilesPath,
        "--metadata",
        metadataPath,
        "--github-output",
        githubOutput,
      ],
      { encoding: "utf8", cwd: REPO_ROOT, env },
    );
    assert.equal(run.status, 0, run.stderr);
    assert.match(run.stdout, /6001 changed file\(s\) classified/);

    const { hits, impact, gates } = select(changedFiles);
    assert.equal(impact.full, false, "the owned files must not force the fallback");
    assert.equal(impact.lanes.wasm, true);
    assert.equal(hits.js, true);
    const expected = formatGithubOutput(gates, impact);
    assert.deepEqual(readFileSync(githubOutput, "utf8").trimEnd().split(/\r?\n/u), expected);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the audit names a path nothing owns and a filter glob that matches nothing", () => {
  const tracked = [
    "crates/verter_wasm/src/lib.rs",
    "docs/guide.md",
    "package.json",
    "brand-new-dir/thing.txt",
    ...Object.values(PATH_FILTERS)
      .flatMap((spec) => (Array.isArray(spec) ? spec : [...spec.include, ...(spec.exclude ?? [])]))
      // One concrete file per glob keeps every glob live but the one dropped.
      .map((glob) => glob.replaceAll("**", "x").replaceAll("*", "x"))
      .filter((file) => file !== "scripts/gate-internals.mjs"),
  ];
  const { unowned, dead, config } = auditSelection(tracked, laneMetadata);
  assert.deepEqual(unowned, ["brand-new-dir/thing.txt"]);
  // No tracked file matches any inert glob here, yet none is reported: an
  // inert glob cannot narrow any lane, so only filter globs are checked.
  assert.deepEqual(dead, ["wasm: scripts/gate-internals.mjs"]);
  assert.deepEqual(config, []);
});

test("a framework vertical's listed contract data runs no lane, and listing it is no hatch", () => {
  // The shape of a framework vertical's pull request: its architecture page
  // and the reviewed contract data it lists as inert.
  const vertical = select([
    "docs/arch/framework-astro.md",
    "tests/framework-astro/AST0/cases.md",
    "tests/framework-astro/AST0/corpus/islands/Counter.astro",
    "tests/framework-astro/AST0/products/astro-capability-matrix.json",
    "tests/framework-liquid/LIQ0/products/liquid-vocabulary.json",
    "tests/framework-ember-glimmer/GLM0/manifest.json",
    "tests/framework-ember-glimmer/GLM0/cases.md",
    "tests/framework-ember-glimmer/GLM0/products/glimmer-version-lock.json",
    "tests/framework-ember-glimmer/GLM0/products/glimmer-capability-matrix.json",
    "tests/framework-ember-glimmer/GLM0/products/glimmer-activation-policy.json",
  ]);
  assert.equal(vertical.impact.full, false, JSON.stringify(vertical.impact.fullReasons));
  assert.deepEqual(gatesOn(vertical.gates), []);
  // An entry names what was reviewed, never a family of future files: an
  // executable spec or a new product beside the listed data still needs a
  // decision, so it forces the fallback (and fails the audit).
  for (const file of [
    "tests/framework-astro/AST0/conformance.spec.mjs",
    "tests/framework-astro/AST0/products/astro-new-product.json",
    "tests/framework-ember-glimmer/GLM0/glimmer-lock.spec.ts",
    "tests/framework-ember-glimmer/GLM0/products/glimmer-new-product.json",
    "tests/framework-newcomer/NEW0/manifest.json",
  ]) {
    assert.equal(isCiInert(file), false, file);
    assert.equal(select([file]).impact.everything, true, file);
  }
  // The inert list is data: editing it runs the classifier's own tests (the
  // js lane), not every lane as an edit to the classifier does.
  for (const file of ["scripts/ci-inert-paths.json", "scripts/ci-inert-paths.d/framework-mdx.json"]) {
    const inertEdit = select([file]);
    assert.equal(inertEdit.impact.full, false, JSON.stringify(inertEdit.impact.fullReasons));
    assert.deepEqual(gatesOn(inertEdit.gates), ["js"], file);
  }
});

test("the tracked tree passes the selection audit", () => {
  // detect-changes runs the same audit (`--audit`) on every change; this is
  // its local run.
  const run = spawnSync(process.execPath, [join(SCRIPT_DIR, "ci-impact.mjs"), "--audit"], {
    encoding: "utf8",
    cwd: REPO_ROOT,
  });
  assert.equal(run.status, 0, `${run.stdout}${run.stderr}`);
  assert.match(run.stdout, /tracked files all owned; every filter glob matches/);
});

test("unreadable selection data breaks no importer: every lane runs and the audit names it", async () => {
  // The sfc-projection filter derives fixtures from the STP1 inventory, and
  // the inert list is data. A change that breaks or moves either must not
  // crash every tool importing this module, and must not narrow selection.
  const stp1 = join(
    "tests",
    "sfc-projection",
    "STP1",
    "products",
    "current-feature-inventory.json",
  );
  const inert = join("scripts", "ci-inert-paths.json");
  for (const [broken, contents, named] of [
    [stp1, "{ not json", /current-feature-inventory\.json/],
    // Every inert entry must say why no ci.yml job reads it.
    [inert, JSON.stringify([{ glob: "docs/**", reason: "" }]), /ci-inert-paths\.json/],
    // A framework vertical's entries live in its own file, so verticals landing
    // together never edit the same list.
    [
      inert,
      JSON.stringify([{ glob: "tests/framework-newcomer/NEW0/cases.md", reason: "Prose." }]),
      /belongs in scripts\/ci-inert-paths\.d\/framework-newcomer\.json/,
    ],
    [
      join("scripts", "ci-inert-paths.d", "framework-mdx.json"),
      JSON.stringify([{ glob: "tests/framework-astro/AST0/cases.md", reason: "Prose." }]),
      /framework-mdx\.json.*belongs in scripts\/ci-inert-paths\.d\/framework-astro\.json/,
    ],
    [join("scripts", "ci-inert-paths.d", "framework-mdx.json"), "{ not json", /framework-mdx\.json/],
  ]) {
    const root = mkdtempSync(join(tmpdir(), "ci-impact-data-"));
    try {
      cpSync(SCRIPT_DIR, join(root, "scripts"), { recursive: true });
      mkdirSync(dirname(join(root, stp1)), { recursive: true });
      cpSync(join(REPO_ROOT, stp1), join(root, stp1));
      writeFileSync(join(root, broken), contents);

      const moved = await import(pathToFileURL(join(root, "scripts", "ci-impact.mjs")).href);
      const audit = moved.auditSelection(["docs/guide.md"], laneMetadata);
      assert.equal(audit.config.length, 1, broken);
      assert.match(audit.config[0], named);

      writeFileSync(join(root, "changed.json"), JSON.stringify(["docs/guide.md"]));
      writeFileSync(join(root, "metadata.json"), JSON.stringify(laneMetadata));
      const githubOutput = join(root, "github-output");
      writeFileSync(githubOutput, "");
      const run = spawnSync(
        process.execPath,
        [
          join(root, "scripts", "ci-impact.mjs"),
          "--changed-files",
          join(root, "changed.json"),
          "--metadata",
          join(root, "metadata.json"),
          "--github-output",
          githubOutput,
        ],
        { encoding: "utf8", cwd: root },
      );
      assert.equal(run.status, 0, `${run.stdout}${run.stderr}`);
      assert.match(
        run.stdout,
        new RegExp(`::error title=ci-impact selection config::.*${named.source}`),
      );
      const gates = readFileSync(githubOutput, "utf8")
        .split(/\r?\n/u)
        .filter((line) => line.startsWith("gate_"));
      assert.ok(
        gates.length === Object.keys(LANE_GATES).length && gates.every((l) => l.endsWith("=true")),
        `${broken}: ${gates.join(" ")}`,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});
