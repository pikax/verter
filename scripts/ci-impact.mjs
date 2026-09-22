#!/usr/bin/env node

/**
 * ci-impact.mjs — dependency-aware lane selection for ci.yml.
 *
 * `detect-changes` decides which jobs a change can affect. For non-Rust
 * inputs that is a path filter (dorny/paths-filter). For Rust inputs a path
 * filter can only say `crates/**`, which runs every artifact lane — the
 * release NAPI binding, the wasm32 build and browser gate, the debug and
 * release LSP builds with the VS Code shards and editor contracts behind
 * them — for a change to a crate none of those artifacts link. This module
 * answers the Rust half from the real dependency graph instead: a lane is
 * impacted when a directly changed crate is in the transitive dependency
 * closure of one of that lane's root crates.
 *
 * Two kinds of gate come out of it (`LANE_GATES`):
 *   - a SUBSYSTEM gate ("the native binding changed", "wasm changed") is the
 *     OR of the lane's path-filter outputs and its impact lanes, and drives
 *     the suites that prove that subsystem;
 *   - an ARTIFACT gate ("the native artifact is needed") is the OR of the
 *     gates of every consumer of that artifact, and drives the producer job,
 *     so a consumer can never be scheduled behind a producer that was not.
 *
 * Fail closed, never under-select:
 *   - `ESCAPE_HATCHES` (workspace manifests and lockfile, nextest config,
 *     the toolchain pin and `.cargo/`, the workflows and local actions, this
 *     classifier and the crate-graph library it uses, proc-macro crates and
 *     the foundational identity crate) turn EVERY impact-bearing lane on;
 *   - a changed file that maps to no crate, is owned by no path filter and is
 *     not on the small explicit CI-inert list (`CI_INERT_PATHS`) does the same;
 *     no directory is inert by assumption — `schemas/` and `test-corpora/` hold
 *     files Rust tests read, so their owner is the rust filter, not silence;
 *   - a lane root that is not a workspace member is a configuration error
 *     and refuses to run rather than yielding an always-false lane;
 *   - if `cargo metadata` itself cannot be read the CLI reports a full
 *     fallback with a warning rather than a narrowed selection.
 *
 * The lane roots are derived from the lanes' own inventories where one
 * exists (the provider selectors, the compile-contract owners), so the test
 * authority and the selection authority cannot drift apart.
 *
 * Usage (CI):
 *   CI_IMPACT_CHANGED_FILES_JSON='[...]' CI_IMPACT_FILTER_OUTPUTS_JSON='{...}' \
 *     node scripts/ci-impact.mjs --github-output "$GITHUB_OUTPUT" --summary "$GITHUB_STEP_SUMMARY"
 * Usage (local, impact lanes only; files no crate owns count as unowned):
 *   node scripts/ci-impact.mjs --range origin/main..HEAD
 */

import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import process from "node:process";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

import {
  buildReverseDependencyGraph,
  buildWorkspaceIndex,
  mapPathToCrate,
  transitiveDependents,
} from "./lib/crate-graph.mjs";
import { PROVIDER_LIVE_SELECTORS } from "./provider-ci-internals.mjs";

/**
 * The compile-contract lane's owners (`node scripts/compile-contracts.mjs
 * --list-owners`) and the crate each owner's fixtures compile against. The
 * runner crate declares these as features with no Cargo edge, so the graph
 * alone cannot see them; `ci-impact.test.mjs` pins this map to the runner's
 * own owner list.
 */
export const COMPILE_CONTRACT_OWNER_CRATES = Object.freeze({
  audit: "verter_audit",
  "compiler-default": "verter_compiler",
  "css-syntax": "verter_css_syntax",
  identity: "verter_identity",
  language: "verter_language",
  semantic: "verter_semantic",
  session: "verter_session",
  "type-runtime": "verter_type_runtime",
  workspace: "verter_workspace",
});

/** Every package the serial provider lanes own tests in. */
export const PROVIDER_PACKAGES = Object.freeze([
  ...new Set(PROVIDER_LIVE_SELECTORS.map((selector) => selector.package)),
]);

/**
 * The crates each impact lane is built from. A lane is impacted when a
 * changed crate is one of these or anything they transitively depend on.
 * Every name must be a workspace member; `classifyCiImpact` refuses otherwise.
 */
export const LANE_ROOTS = Object.freeze({
  // The release N-API binding (`build-native-node`) and its consumers.
  native: ["verter_napi"],
  // The wasm32 artifact (`wasm-build`) and the browser gate behind it.
  wasm: ["verter_wasm"],
  // The debug LSP binaries: VS Code E2E, editor-neutral contract, DX harness.
  lsp: ["verter_lsp", "verter_relay_shim", "verter_mcp", "verter_dx_baseline"],
  // The release LSP binary plus the editor-client shipping plans.
  editors: ["verter_lsp", "verter-editor-client"],
  // The serial real-provider lane: every package its selectors own.
  providers: [...PROVIDER_PACKAGES],
  // Compile-fail contracts: the runner crates plus every owner whose fixtures
  // the runner compiles, the isolated verter_compiler build, and the
  // generated Svelte artifact check.
  compiler_contracts: [
    "verter_compile_contracts",
    "verter_compile_contracts_bench",
    "verter_compile_contracts_session_variants",
    ...new Set(Object.values(COMPILE_CONTRACT_OWNER_CRATES)),
  ],
  // The feature-gated BF2 inventory is verter_session's lib under a feature.
  bf2: ["verter_session"],
  svelte_conformance: ["verter_svelte_conformance"],
  // The PR-side Svelte benchmark contract proves the corpus against the
  // compiler; the measurement itself runs nightly.
  svelte_perf: ["verter_compiler"],
  // The playground build consumes the wasm and native artifacts.
  playground: ["verter_wasm", "verter_napi"],
});

/**
 * The job gates `detect-changes` publishes, in evaluation order. A gate is
 * the OR of the named paths-filter outputs (`filters`), the named impact
 * lanes (`impact`) and previously evaluated gates (`gates`). A gate with
 * only `filters` is a pure pass-through of its filter.
 */
export const LANE_GATES = Object.freeze({
  rust: { filters: ["rust"] },
  proto: { filters: ["proto"] },
  js: { filters: ["js"] },
  arch: { filters: ["arch"] },
  svelte_oracle: { filters: ["svelte_oracle"] },
  svelte_client_smokes: { filters: ["svelte_client_smokes"] },
  jetbrains: { filters: ["jetbrains"] },
  // Subsystem gates.
  wasm: { filters: ["wasm"], impact: ["wasm"] },
  native: { filters: ["js"], impact: ["native"] },
  playground: { filters: ["playground"], impact: ["playground"] },
  // Transport equivalence compares the two artifacts, so either subsystem
  // moving demands BOTH artifacts (see the artifact gates below).
  transport: { filters: ["transport"], impact: ["native", "wasm"] },
  // Artifact gates: a producer runs whenever any consumer of it runs.
  native_artifact: { gates: ["native", "playground", "transport"] },
  wasm_artifact: { gates: ["wasm", "playground", "transport"] },
  vscode: { filters: ["vscode"], impact: ["lsp"] },
  dx: { filters: ["dx"], impact: ["lsp"] },
  editor_lsp: { filters: ["editor_lsp"], impact: ["lsp"] },
  editors: { filters: ["editors"], impact: ["editors"] },
  providers: { filters: ["providers"], impact: ["providers"] },
  compiler_contracts: { filters: ["compiler_contracts"], impact: ["compiler_contracts"] },
  bf2: { filters: ["bf2"], impact: ["bf2"] },
  svelte_conformance: { filters: ["svelte_oracle"], impact: ["svelte_conformance"] },
  svelte_perf: { filters: ["svelte_perf"], impact: ["svelte_perf"] },
});

/**
 * This classifier's own escape hatches. Narrower than `affected-tests.mjs`'s:
 * CI has explicit path owners, so `scripts/`, `packages/`, the JS install
 * graph and the rest are handled by the lane whose filter names them (an
 * unowned one still falls back, below). What stays global is what changes
 * how every crate compiles, what runs, or how this selection is made.
 */
export const ESCAPE_HATCHES = Object.freeze([
  {
    id: "workspace-manifest",
    reason: "the workspace manifest or lockfile changes resolution for every crate",
    test: (p) => p === "Cargo.toml" || p === "Cargo.lock",
  },
  {
    id: "nextest-config",
    reason: "nextest.toml controls test execution semantics for every nextest lane",
    test: (p) => p === ".config/nextest.toml",
  },
  {
    id: "toolchain",
    reason: "the pinned toolchain and cargo configuration change how every crate builds",
    test: (p) => p === "rust-toolchain.toml" || p === ".cargo" || p.startsWith(".cargo/"),
  },
  {
    id: "ci-workflows",
    reason: "workflow definitions decide what runs; a selector cannot reason about them",
    test: (p) => p === ".github/workflows" || p.startsWith(".github/workflows/"),
  },
  {
    id: "ci-actions",
    reason: "local composite actions are steps of every job that uses them",
    test: (p) => p === ".github/actions" || p.startsWith(".github/actions/"),
  },
  {
    id: "lane-classifier",
    reason: "the selection logic itself, or the crate-graph library it is built on, changed",
    test: (p) => p === "scripts/ci-impact.mjs" || p === "scripts/lib/crate-graph.mjs",
  },
  {
    id: "proc-macro-crate",
    reason:
      "a proc-macro crate expands into every consumer at build time, not along a linking edge",
    test: (p, index) => Boolean(mapPathToCrate(index, p)?.isProcMacro),
  },
  {
    id: "verter-identity",
    reason: "verter_identity is foundational; identity semantics reach every crate",
    test: (p) => p === "crates/verter_identity" || p.startsWith("crates/verter_identity/"),
  },
]);

/**
 * Files that positively have no CI consumer. This is NOT the broad
 * `KNOWN_NON_RUST_TOP_LEVEL` of `affected-tests.mjs`: that list assumes whole
 * directories are irrelevant to the Rust build, which is a fine default for a
 * local inner-loop selector and an under-selection for CI, where
 * `schemas/**` is validated by a Rust test and `test-corpora/**` is read by
 * Rust tests and `include_str!`. A file that no crate owns, no path filter
 * owns and this list does not name falls back to the full graph.
 */
export const CI_INERT_PATHS = Object.freeze([
  "docs/",
  ".claude/",
  ".husky/",
  ".vscode/",
  "CHANGELOG.md",
  "CONTRIBUTING.md",
  "AGENTS.md",
  "CLAUDE.md",
  "LICENSE",
  "readme.md",
  "README.md",
  "cliff.toml",
  "netlify.toml",
  ".gitignore",
]);

export function isCiInert(relPath) {
  return CI_INERT_PATHS.some((entry) =>
    entry.endsWith("/") ? relPath.startsWith(entry) : relPath === entry,
  );
}

function matchHatch(relPath, index) {
  for (const rule of ESCAPE_HATCHES) {
    if (rule.test(relPath, index)) return rule;
  }
  return null;
}

/**
 * Pure decision core.
 *
 * @param {string[]} changedFiles workspace-relative, forward-slash paths
 * @param {object} metadata parsed `cargo metadata --format-version=1`
 * @param {Record<string, string[]>} laneRoots
 * @param {{ownedFiles?: Set<string> | null}} [options] files some path filter
 *   matched; a non-crate file outside this set and outside `CI_INERT_PATHS`
 *   forces the full fallback. `null`/absent means nothing is known to be owned.
 */
export function classifyCiImpact(changedFiles, metadata, laneRoots = LANE_ROOTS, options = {}) {
  const ownedFiles = options.ownedFiles ?? new Set();
  const index = buildWorkspaceIndex(metadata);
  for (const [lane, roots] of Object.entries(laneRoots)) {
    for (const root of roots) {
      if (!index.byName.has(root)) {
        throw new Error(
          `ci-impact: lane "${lane}" root "${root}" is not a workspace member; fix LANE_ROOTS`,
        );
      }
    }
  }
  const reverse = buildReverseDependencyGraph(metadata, index);

  const fullReasons = [];
  const directCrates = new Set();
  const ownedNonRust = [];
  for (const file of changedFiles) {
    const hatch = matchHatch(file, index);
    if (hatch) {
      fullReasons.push({ file, id: hatch.id, reason: hatch.reason });
      continue;
    }
    const crate = mapPathToCrate(index, file);
    if (crate) {
      directCrates.add(crate.name);
      continue;
    }
    if (ownedFiles.has(file) || isCiInert(file)) {
      ownedNonRust.push(file);
      continue;
    }
    fullReasons.push({
      file,
      id: "unrecognized-path",
      reason: "no crate owns it and no path filter matched it; over-select rather than guess",
    });
  }

  const full = fullReasons.length > 0;
  const impacted = full ? new Set() : transitiveDependents(reverse, directCrates);
  const lanes = {};
  for (const [lane, roots] of Object.entries(laneRoots)) {
    lanes[lane] = full || roots.some((root) => impacted.has(root));
  }
  return {
    full,
    fullReasons,
    directCrates: [...directCrates].sort(),
    impactedCrates: [...impacted].sort(),
    ownedNonRust,
    lanes,
  };
}

function truthy(value) {
  return value === true || value === "true";
}

/**
 * @param {Record<string, string>} filterOutputs `steps.filter.outputs` as JSON
 * @param {{full: boolean, lanes: Record<string, boolean>}} impact
 * @param {typeof LANE_GATES} gates
 * @returns {Record<string, "true" | "false">}
 */
export function composeLaneGates(filterOutputs, impact, gates = LANE_GATES) {
  const result = {};
  for (const [gate, spec] of Object.entries(gates)) {
    let on = false;
    for (const filter of spec.filters ?? []) {
      if (!(filter in filterOutputs)) {
        throw new Error(
          `ci-impact: gate "${gate}" names filter "${filter}" is not among the paths-filter outputs`,
        );
      }
      if (truthy(filterOutputs[filter])) on = true;
    }
    for (const lane of spec.impact ?? []) {
      if (impact.full) {
        on = true;
        continue;
      }
      if (!(lane in impact.lanes)) {
        throw new Error(`ci-impact: gate "${gate}" names unknown impact lane "${lane}"`);
      }
      if (impact.lanes[lane]) on = true;
    }
    for (const other of spec.gates ?? []) {
      if (!(other in result)) {
        throw new Error(
          `ci-impact: gate "${gate}" depends on gate "${other}", which is not evaluated before it`,
        );
      }
      if (result[other] === "true") on = true;
    }
    result[gate] = on ? "true" : "false";
  }
  return result;
}

/** The files the paths-filter step matched, from its `<filter>_files` outputs. */
export function ownedFilesFromFilterOutputs(filterOutputs) {
  const owned = new Set();
  for (const [key, value] of Object.entries(filterOutputs)) {
    if (!key.endsWith("_files") || key === "any_files") continue;
    let files;
    try {
      files = JSON.parse(value);
    } catch {
      throw new Error(`ci-impact: paths-filter output ${key} is not a JSON file list`);
    }
    for (const file of files) owned.add(String(file).replace(/\\/g, "/"));
  }
  return owned;
}

/** `$GITHUB_OUTPUT` lines: one `gate_<name>` per gate, then the fallback flag. */
export function formatGithubOutput(gates, impact) {
  return [
    ...Object.entries(gates).map(([gate, value]) => `gate_${gate}=${value}`),
    `impact_full=${impact.full ? "true" : "false"}`,
  ];
}

function summaryMarkdown(changedFiles, impact, gates) {
  const lines = ["### CI lane selection", ""];
  if (impact.full) {
    lines.push("**Full fallback** — every impact-bearing lane runs:", "");
    for (const r of impact.fullReasons) lines.push(`- \`${r.file}\` (${r.id}): ${r.reason}`);
  } else {
    lines.push(
      `Changed crates: ${impact.directCrates.map((c) => `\`${c}\``).join(", ") || "none"}`,
      "",
      `Impacted (changed + dependents): ${impact.impactedCrates.length} crate(s)`,
    );
  }
  lines.push("", "| Gate | Runs |", "| --- | --- |");
  for (const [gate, value] of Object.entries(gates)) lines.push(`| ${gate} | ${value} |`);
  lines.push("", `${changedFiles.length} changed file(s) classified.`, "");
  return lines.join("\n");
}

function loadCargoMetadata(cwd) {
  const raw = execFileSync(
    "cargo",
    ["metadata", "--format-version=1", "--all-features", "--locked"],
    { cwd, maxBuffer: 1024 * 1024 * 256, stdio: ["ignore", "pipe", "inherit"] },
  );
  return JSON.parse(raw.toString("utf8"));
}

function changedFilesFromRange(cwd, range) {
  return execFileSync("git", ["diff", "--no-renames", "--name-only", range], {
    cwd,
    maxBuffer: 1024 * 1024 * 64,
  })
    .toString("utf8")
    .split("\n")
    .filter(Boolean)
    .map((f) => f.replace(/\\/g, "/"));
}

function argValue(argv, flag) {
  const index = argv.indexOf(flag);
  return index >= 0 ? argv[index + 1] : undefined;
}

export function main(argv = process.argv.slice(2), env = process.env, cwd = process.cwd()) {
  const range = argValue(argv, "--range");
  const githubOutput = argValue(argv, "--github-output");
  const summaryPath = argValue(argv, "--summary");
  const metadataPath = argValue(argv, "--metadata");

  let changedFiles;
  if (range) {
    changedFiles = changedFilesFromRange(cwd, range);
  } else if (env.CI_IMPACT_CHANGED_FILES_JSON) {
    changedFiles = JSON.parse(env.CI_IMPACT_CHANGED_FILES_JSON);
  } else {
    process.stderr.write("ci-impact: pass --range <rev>..<rev> or CI_IMPACT_CHANGED_FILES_JSON\n");
    return 2;
  }
  if (!Array.isArray(changedFiles) || !changedFiles.every((f) => typeof f === "string")) {
    process.stderr.write("ci-impact: the changed-file list must be a JSON array of paths\n");
    return 2;
  }
  changedFiles = changedFiles.map((f) => f.replace(/\\/g, "/"));

  const filterOutputs = env.CI_IMPACT_FILTER_OUTPUTS_JSON
    ? JSON.parse(env.CI_IMPACT_FILTER_OUTPUTS_JSON)
    : null;
  const ownedFiles = filterOutputs ? ownedFilesFromFilterOutputs(filterOutputs) : null;

  let impact;
  try {
    const metadata = metadataPath
      ? JSON.parse(readFileSync(resolve(cwd, metadataPath), "utf8"))
      : loadCargoMetadata(cwd);
    impact = classifyCiImpact(changedFiles, metadata, LANE_ROOTS, { ownedFiles });
  } catch (error) {
    if (/lane root|LANE_ROOTS/.test(String(error?.message))) {
      // A stale root is a configuration bug: fail the job so it gets fixed.
      process.stderr.write(`${error.message}\n`);
      return 1;
    }
    // Anything else (cargo unavailable, registry outage) is an unknown input:
    // fall back to the full graph rather than narrow on a guess.
    process.stdout.write(
      `::warning title=ci-impact full fallback::could not derive the crate graph: ${String(
        error?.message ?? error,
      )}\n`,
    );
    impact = {
      full: true,
      fullReasons: [{ file: "(cargo metadata)", id: "graph-unavailable", reason: String(error) }],
      directCrates: [],
      impactedCrates: [],
      ownedNonRust: [],
      lanes: Object.fromEntries(Object.keys(LANE_ROOTS).map((lane) => [lane, true])),
    };
  }

  let gates;
  if (filterOutputs) {
    gates = composeLaneGates(filterOutputs, impact);
  } else {
    // Local use: report the impact lanes alone.
    gates = Object.fromEntries(
      Object.entries(impact.lanes).map(([lane, on]) => [lane, on ? "true" : "false"]),
    );
  }

  const lines = formatGithubOutput(gates, impact);
  if (githubOutput) appendFileSync(githubOutput, `${lines.join("\n")}\n`);
  if (summaryPath) appendFileSync(summaryPath, `${summaryMarkdown(changedFiles, impact, gates)}\n`);
  process.stdout.write(`${summaryMarkdown(changedFiles, impact, gates)}\n`);
  return 0;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  process.exitCode = main();
}
