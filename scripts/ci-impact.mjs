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
 * Fail closed, never under-select:
 *   - every escape hatch of `scripts/lib/crate-graph.mjs` (workspace
 *     manifests and lockfile, nextest config, scripts/, proc-macro crates,
 *     generated bindings, the toolchain pin, `.cargo/`, the workflows and the
 *     local actions) turns EVERY impact-bearing lane on;
 *   - so does a changed path this module cannot classify;
 *   - a lane root that is not a workspace member is a configuration error
 *     and refuses to run rather than yielding an always-false lane;
 *   - if `cargo metadata` itself cannot be read the CLI reports a full
 *     fallback with a warning rather than a narrowed selection.
 *
 * The final job gates are `composeLaneGates`: each gate is the OR of its
 * path-filter outputs and its impact lanes, so the path filters keep owning
 * every non-crate input and this module owns only `crates/**`.
 *
 * Usage (CI):
 *   CI_IMPACT_CHANGED_FILES_JSON='[...]' CI_IMPACT_FILTER_OUTPUTS_JSON='{...}' \
 *     node scripts/ci-impact.mjs --github-output "$GITHUB_OUTPUT" --summary "$GITHUB_STEP_SUMMARY"
 * Usage (local):
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
  classifyChangedFile,
  transitiveDependents,
} from "./lib/crate-graph.mjs";

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
  // The serial real-provider lanes (tsserver, tsgo).
  providers: ["verter_lsp", "verter_type_runtime", "verter_tsgo_api"],
  // Compile-fail contracts, the isolated verter_compiler build, and the
  // generated Svelte artifact check.
  compiler_contracts: [
    "verter_compile_contracts",
    "verter_compile_contracts_bench",
    "verter_compile_contracts_session_variants",
    "verter_compiler",
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
 * The job gates `detect-changes` publishes: each is the OR of the named
 * paths-filter outputs and the named impact lanes. A gate with no `impact`
 * is a pure pass-through of its filter.
 */
export const LANE_GATES = Object.freeze({
  rust: { filters: ["rust"] },
  proto: { filters: ["proto"] },
  js: { filters: ["js"] },
  arch: { filters: ["arch"] },
  svelte_oracle: { filters: ["svelte_oracle"] },
  svelte_client_smokes: { filters: ["svelte_client_smokes"] },
  jetbrains: { filters: ["jetbrains"] },
  wasm: { filters: ["wasm"], impact: ["wasm"] },
  native: { filters: ["js", "playground"], impact: ["native"] },
  playground: { filters: ["playground"], impact: ["playground"] },
  vscode: { filters: ["vscode"], impact: ["lsp"] },
  dx: { filters: ["dx"], impact: ["lsp"] },
  editor_lsp: { filters: ["editor_lsp"], impact: ["lsp"] },
  editors: { filters: ["editors"], impact: ["editors"] },
  providers: { filters: ["providers"], impact: ["providers"] },
  compiler_contracts: { filters: [], impact: ["compiler_contracts"] },
  bf2: { filters: ["bf2"], impact: ["bf2"] },
  svelte_conformance: { filters: ["svelte_oracle"], impact: ["svelte_conformance"] },
  svelte_perf: { filters: ["svelte_perf"], impact: ["svelte_perf"] },
});

/**
 * Pure decision core.
 *
 * @param {string[]} changedFiles workspace-relative, forward-slash paths
 * @param {object} metadata parsed `cargo metadata --format-version=1`
 * @param {Record<string, string[]>} laneRoots
 * @returns {{
 *   full: boolean,
 *   fullReasons: Array<{file: string, id: string, reason: string}>,
 *   directCrates: string[],
 *   impactedCrates: string[],
 *   lanes: Record<string, boolean>,
 * }}
 */
export function classifyCiImpact(changedFiles, metadata, laneRoots = LANE_ROOTS) {
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
  for (const file of changedFiles) {
    const classification = classifyChangedFile(index, file);
    switch (classification.kind) {
      case "escape-hatch":
        fullReasons.push({ file, id: classification.id, reason: classification.reason });
        break;
      case "crate":
        directCrates.add(classification.name);
        break;
      case "ignored":
        break;
      case "unrecognized":
        fullReasons.push({
          file,
          id: "unrecognized-path",
          reason: "does not map to a workspace crate or a known non-Rust path; over-select",
        });
        break;
      default:
        throw new Error(`unreachable classification kind: ${classification.kind}`);
    }
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
    result[gate] = on ? "true" : "false";
  }
  return result;
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

  let impact;
  try {
    const metadata = metadataPath
      ? JSON.parse(readFileSync(resolve(cwd, metadataPath), "utf8"))
      : loadCargoMetadata(cwd);
    impact = classifyCiImpact(changedFiles, metadata);
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
      lanes: Object.fromEntries(Object.keys(LANE_ROOTS).map((lane) => [lane, true])),
    };
  }

  let gates;
  if (env.CI_IMPACT_FILTER_OUTPUTS_JSON) {
    gates = composeLaneGates(JSON.parse(env.CI_IMPACT_FILTER_OUTPUTS_JSON), impact);
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
