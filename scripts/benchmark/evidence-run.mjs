#!/usr/bin/env node
// Evidence runs: named semantic, soak and formatter-contract invocations. A manifest
// (scripts/benchmark/evidence-runs/<name>.json) fixes what runs and which
// cells must come back; this runner validates it, drives semantic-perf.mjs
// with the manifest's options, re-validates the result with the harness's
// own validate.mjs and writes one machine-readable summary. It adds no
// second harness, classifier or validator.
//
//   node scripts/benchmark/evidence-run.mjs --run <name> --dry-run
//   node scripts/benchmark/evidence-run.mjs --run <name> [--worker <record.json>]
//
// See scripts/benchmark/evidence-runs/README.md.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { ARMS } from "./semantic-perf/analyze.mjs";
import { harnessFingerprint, resolveTypeScript } from "./semantic-perf/provenance.mjs";
import { scenariosForTier, selectScenarios, SETTINGS, TIERS } from "./semantic-perf/scenarios.mjs";
import { allSessions } from "./semantic-perf/sessions.mjs";
import {
  buildFormatterSummary,
  validateFormatterManifest,
} from "../../tests/formatter/evidence.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
export const ROOT = resolve(HERE, "..", "..");
export const MANIFEST_DIR = join(HERE, "evidence-runs");
export const NOISE_DOC = "docs/contributing/semantic-benchmark.md";
export const SUMMARY_FILE = "evidence-summary.json";
export const SUMMARY_SCHEMA = 1;
export const BENCH_TAG = "bench-m3";

const REQUIRED_FIELDS = [
  "name",
  "description",
  "tier",
  "arms",
  "modes",
  "threads",
  "noise",
  "requiredCells",
];
const OPTIONAL_FIELDS = [
  "scenarios",
  "sessions",
  "settings",
  "repeat",
  "warmup",
  "warmRepeats",
  "requireAllMatched",
];
const CELL_FIELDS = ["scenario", "setting", "arm", "mode", "threads", "metrics"];

/** The workload modes the harness measures: a fresh-process cold demand and in-process warm repeats. */
export const MODES = ["cold", "warm"];

/**
 * The thread count each arm runs at. The harness takes no thread option:
 * every arm runs at its tool's default, except `tsc -p --singleThreaded`.
 */
export const ARM_THREADS = Object.fromEntries(
  Object.keys(ARMS).map((arm) => [arm, arm === "tsc-cli-1" ? 1 : "default"]),
);

/**
 * The metrics a required cell may name, per arm kind and mode, each with its
 * kind: `time` and `memory` are measured only on bench-m3; `work` (the
 * counting allocator's deterministic counts) on any worker. `from` reads the
 * value out of the harness's per-arm summary.
 */
const probeMetric = (kind) => ({ kind, from: (arm, m) => arm.metrics?.[m] ?? null });
const PROBE_COLD = {
  engineStartMs: probeMetric("time"),
  setupMs: probeMetric("time"),
  initMs: probeMetric("time"),
  coldMs: probeMetric("time"),
  firstTypeMs: probeMetric("time"),
  observeMs: probeMetric("time"),
  teardownMs: probeMetric("time"),
  cpuMs: probeMetric("time"),
  peakBytes: probeMetric("memory"),
  retainedBytes: probeMetric("memory"),
  observePeakBytes: probeMetric("memory"),
};
const COUNTED_COLD = {
  coldAllocations: { kind: "work", from: (arm) => arm.coldAllocations ?? null },
  coldAllocatedBytes: { kind: "work", from: (arm) => arm.coldAllocatedBytes ?? null },
};
const cliMetric = (kind) => ({ kind, from: (arm, m) => arm[m] ?? null });
const CLI_COLD = {
  wallMs: cliMetric("time"),
  cpuMs: cliMetric("time"),
  tscCheckMs: cliMetric("time"),
  tscTotalMs: cliMetric("time"),
  peakBytes: cliMetric("memory"),
  tscMemoryUsedBytes: cliMetric("memory"),
};

/** The metrics (name -> definition) an (arm, mode) cell carries; empty when the arm has no such mode. */
export function metricsFor(arm, mode) {
  const def = ARMS[arm];
  if (!def) return {};
  if (def.kind === "cli") return mode === "cold" ? CLI_COLD : {};
  if (mode === "warm") return { warmMs: probeMetric("time") };
  if (mode !== "cold") return {};
  return arm === "verter-counted" ? { ...PROBE_COLD, ...COUNTED_COLD } : PROBE_COLD;
}

/** GitHub-style anchors of a Markdown file's headings. */
function headingAnchors(file) {
  const anchors = new Set();
  for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
    const m = /^#{1,6}\s+(.*)$/.exec(line);
    if (m)
      anchors.add(
        m[1]
          .trim()
          .toLowerCase()
          .replace(/[^\p{L}\p{N}\s-]/gu, "")
          .replace(/\s/g, "-"),
      );
  }
  return anchors;
}

const isInt = (v, min) => Number.isInteger(v) && v >= min;
const stringList = (v) =>
  Array.isArray(v) && v.length > 0 && v.every((x) => typeof x === "string" && x);

/**
 * Validate a parsed manifest loaded from `file`. Returns `{ problems, run }`:
 * `run` (the resolved scenarios, settings and harness arguments) only when
 * there are no problems.
 */
export function validateManifest(manifest, file) {
  const problems = [];
  if (!manifest || typeof manifest !== "object" || Array.isArray(manifest))
    return { problems: ["the manifest is not a JSON object"], run: null };
  if ("kind" in manifest)
    return manifest.kind === "formatter"
      ? validateFormatterManifest(manifest, file)
      : manifest.kind === "soak"
        ? validateSoakManifest(manifest, file)
        : {
            problems: [`unknown kind ${JSON.stringify(manifest.kind)}; kinds: soak, formatter`],
            run: null,
          };
  for (const key of Object.keys(manifest))
    if (!REQUIRED_FIELDS.includes(key) && !OPTIONAL_FIELDS.includes(key))
      problems.push(`unknown field ${key}`);
  for (const key of REQUIRED_FIELDS)
    if (!(key in manifest)) problems.push(`missing required field ${key}`);
  if (problems.length) return { problems, run: null };

  const { name, tier, arms, modes, threads, noise, requiredCells } = manifest;
  const stem = file ? file.replace(/^.*[\\/]/, "").replace(/\.json$/, "") : name;
  if (typeof name !== "string" || !/^[a-z0-9][a-z0-9-]*$/.test(name))
    problems.push(`name ${JSON.stringify(name)} must be lower-case letters, digits and hyphens`);
  else if (name !== stem) problems.push(`name ${name} differs from the file name ${stem}.json`);
  if (typeof manifest.description !== "string" || !manifest.description.trim())
    problems.push("description must be a non-empty string");
  if (!TIERS.includes(tier))
    problems.push(`tier ${JSON.stringify(tier)} is not one of ${TIERS.join(", ")}`);

  let scenarios = [];
  if ("scenarios" in manifest) {
    if (!stringList(manifest.scenarios))
      problems.push("scenarios must be a non-empty list of ids or prefixes");
    else
      for (const prefix of manifest.scenarios) {
        try {
          scenarios.push(
            ...selectScenarios([prefix]).filter((s) => !scenarios.some((e) => e.id === s.id)),
          );
        } catch {
          problems.push(`scenario ${prefix} selects no scenario of the catalog`);
        }
      }
  } else if (TIERS.includes(tier)) scenarios = scenariosForTier(tier);
  let sessions = [];
  if ("sessions" in manifest) {
    const catalog = new Map(allSessions().map((s) => [s.id, s]));
    if (!stringList(manifest.sessions)) problems.push("sessions must be a non-empty list of ids");
    else {
      if (new Set(manifest.sessions).size !== manifest.sessions.length)
        problems.push("sessions lists a session twice");
      for (const id of manifest.sessions) {
        const session = catalog.get(id);
        if (!session) problems.push(`session ${id} is not a session of the catalog`);
        else if (!sessions.includes(session)) sessions.push(session);
      }
      // A retention proof compares a growth session with its baseline: the
      // run must measure both.
      for (const session of sessions)
        if (session.retentionProof && !manifest.sessions.includes(session.retentionProof.baseline))
          problems.push(
            `session ${session.id} is proved against ${session.retentionProof.baseline}, which sessions does not list`,
          );
      // A selected session runs only through --only, which then also
      // selects scenarios by prefix: list them explicitly.
      if (!("scenarios" in manifest)) scenarios = [];
    }
  }
  const scenarioIds = new Set(scenarios.map((s) => s.id));

  // The harness runs the strict setting alone or all four.
  const allSettings = SETTINGS.map((s) => s.id);
  let settings = ["strict"];
  if ("settings" in manifest) {
    const given = manifest.settings;
    if (!stringList(given)) problems.push("settings must be a non-empty list of setting ids");
    else {
      for (const s of given) if (!allSettings.includes(s)) problems.push(`unknown setting ${s}`);
      const set = new Set(given);
      if (set.size !== given.length) problems.push("settings lists a setting twice");
      else if (given.every((s) => allSettings.includes(s))) {
        if (set.size === 1 && set.has("strict")) settings = ["strict"];
        else if (set.size === allSettings.length) settings = allSettings;
        else
          problems.push(
            `settings ${given.join(",")}: the harness runs either strict alone or all of ${allSettings.join(", ")}`,
          );
      }
    }
  }

  if (!stringList(arms)) problems.push("arms must be a non-empty list of arm ids");
  else {
    for (const arm of arms)
      if (!ARMS[arm]) problems.push(`unknown arm ${arm}; arms: ${Object.keys(ARMS).join(", ")}`);
    if (new Set(arms).size !== arms.length) problems.push("arms lists an arm twice");
  }
  if (!stringList(modes)) problems.push("modes must be a non-empty list of modes");
  else {
    for (const mode of modes)
      if (!MODES.includes(mode)) problems.push(`unknown mode ${mode}; modes: ${MODES.join(", ")}`);
    if (new Set(modes).size !== modes.length) problems.push("modes lists a mode twice");
  }
  const runArms = stringList(arms) ? arms.filter((a) => ARMS[a]) : [];
  if (
    !Array.isArray(threads) ||
    !threads.length ||
    !threads.every((t) => t === "default" || isInt(t, 1))
  )
    problems.push('threads must be a non-empty list of "default" or positive thread counts');
  else {
    if (new Set(threads).size !== threads.length)
      problems.push("threads lists a thread count twice");
    for (const arm of runArms)
      if (!threads.includes(ARM_THREADS[arm]))
        problems.push(
          `arm ${arm} runs at threads ${ARM_THREADS[arm]}, which threads does not list`,
        );
    for (const t of threads)
      if (!runArms.some((arm) => ARM_THREADS[arm] === t))
        problems.push(`threads ${t}: no arm of the run runs at that thread count`);
  }
  for (const [key, min] of [
    ["repeat", 2],
    ["warmup", 0],
    ["warmRepeats", 1],
  ])
    if (key in manifest && !isInt(manifest[key], min))
      problems.push(`${key} must be an integer >= ${min}`);
  if ("requireAllMatched" in manifest && typeof manifest.requireAllMatched !== "boolean")
    problems.push("requireAllMatched must be true or false");

  const noiseRefs = typeof noise === "string" ? [noise] : noise;
  if (!stringList(noiseRefs)) problems.push("noise must be a reference or a list of references");
  else {
    const anchors = headingAnchors(join(ROOT, NOISE_DOC));
    for (const ref of noiseRefs) {
      const [doc, anchor] = ref.split("#");
      if (doc !== NOISE_DOC || !anchor || !anchors.has(anchor))
        problems.push(`noise reference ${ref} is not a section of ${NOISE_DOC}`);
    }
  }

  if (!Array.isArray(requiredCells) || !requiredCells.length)
    problems.push("requiredCells must be a non-empty list");
  else {
    const seen = new Set();
    requiredCells.forEach((cell, i) => {
      const at = `requiredCells[${i}]`;
      if (!cell || typeof cell !== "object" || Array.isArray(cell)) {
        problems.push(`${at} is not an object`);
        return;
      }
      for (const key of Object.keys(cell))
        if (!CELL_FIELDS.includes(key)) problems.push(`${at}: unknown field ${key}`);
      for (const key of CELL_FIELDS)
        if (!(key in cell)) problems.push(`${at}: missing field ${key}`);
      const { scenario, setting, arm, mode, threads: t, metrics } = cell;
      if (!scenarioIds.has(scenario))
        problems.push(`${at}: scenario ${scenario} is not one of the run's scenarios`);
      if (!settings.includes(setting))
        problems.push(`${at}: setting ${setting} is not one of the run's settings`);
      if (!runArms.includes(arm)) problems.push(`${at}: arm ${arm} is not one of the run's arms`);
      if (!Array.isArray(modes) || !modes.includes(mode))
        problems.push(`${at}: mode ${mode} is not one of the run's modes`);
      if (ARMS[arm] && t !== ARM_THREADS[arm])
        problems.push(`${at}: arm ${arm} runs at threads ${ARM_THREADS[arm]}, not ${t}`);
      const defined = metricsFor(arm, mode);
      if (ARMS[arm] && MODES.includes(mode) && !Object.keys(defined).length)
        problems.push(`${at}: arm ${arm} has no ${mode} mode`);
      if (!stringList(metrics)) problems.push(`${at}: metrics must be a non-empty list`);
      else
        for (const m of metrics)
          if (Object.keys(defined).length && !defined[m])
            problems.push(`${at}: arm ${arm} has no ${mode} metric ${m}`);
      const key = cellKey(cell);
      if (seen.has(key)) problems.push(`${at}: duplicate cell ${key}`);
      seen.add(key);
    });
  }
  if (problems.length) return { problems, run: null };

  const only = [...(manifest.scenarios ?? []), ...(manifest.sessions ?? [])];
  const run = {
    scenarios: scenarios.map((s) => s.id),
    sessions: sessions.map((s) => s.id),
    settings,
    requireAllMatched: manifest.requireAllMatched === true,
    harnessArgs: [
      "--tier",
      tier,
      ...(only.length ? ["--only", only.join(",")] : []),
      "--settings",
      settings.length === 1 ? "strict" : "all",
      "--arms",
      arms.join(","),
      ...[
        ["repeat", "--repeat"],
        ["warmup", "--warmup"],
        ["warmRepeats", "--warm-repeats"],
      ].flatMap(([key, flag]) => (key in manifest ? [flag, String(manifest[key])] : [])),
    ],
  };
  return { problems, run };
}

/**
 * A soak manifest (`kind: "soak"`) registers a frozen run of the long-churn
 * soak runner (`crates/verter_bench/src/soak/`, binary `verter-soak`) instead
 * of a semantic-perf harness invocation. Its workloads are frozen by digest:
 * the runner's own test regenerates them and compares, so the manifest is the
 * single record of what the run executes.
 */
export const SOAK_MODES = ["normal", "pressure"];
const SOAK_REQUIRED = ["name", "kind", "description", "modes", "noise", "soak", "requiredCells"];
const SOAK_FIELDS = [
  "surgeLimits",
  "runner",
  "tranches",
  "actionsPerTranche",
  "minDistinctQueries",
  "sampleEvery",
  "envelopeActions",
  "workloads",
  "smoke",
  "controls",
  "binding",
  "build",
];
/**
 * The one build the soak runs on, on every worker: the runner binary of
 * `verter_bench` with its default-off `soak-support` tooling feature and no
 * other feature (audit/footprint capture and `semantic-observe` stay off).
 */
export const SOAK_BUILD = {
  package: "verter_bench",
  bin: "verter-soak",
  profile: "release",
  features: ["soak-support"],
};
const SOAK_COUNTS = [
  "tranches",
  "actionsPerTranche",
  "minDistinctQueries",
  "sampleEvery",
  "envelopeActions",
];
const SOAK_CELL_FIELDS = ["mode", "metrics", "benchM3Only"];

const closedCounts = (r) => r.summary?.samples?.find((s) => s.at === "closed")?.counts ?? null;
const finalCounts = (r) => r.summary?.samples?.at(-1)?.counts ?? null;
const surges = (r) => (r.summary?.records ?? []).filter((x) => x.class === "pressure-surge");
const surgeSum = (r, pick) =>
  surges(r).length ? surges(r).reduce((sum, s) => sum + (pick(s) ?? 0), 0) : null;
const surgeMax = (r, pick) => {
  const values = surges(r)
    .map(pick)
    .filter((v) => typeof v === "number");
  return values.length ? Math.max(...values) : null;
};

/**
 * The metrics a soak cell may name. `work` metrics are the runner's
 * deterministic verdict and counts, read on any worker; `time` and `memory`
 * only on bench-m3, so a cell naming one is marked `benchM3Only`. `from`
 * reads the value from one mode's `{ verdict, summary, wallMs }`.
 */
export const SOAK_METRICS = {
  verdict: { kind: "work", from: (r) => r.verdict?.verdict ?? null },
  // The negative controls run once, before the modes, and are shared by both.
  controls: { kind: "work", from: (r) => r.controls?.verdict ?? null },
  failures: { kind: "work", from: (r) => r.verdict?.failures?.length ?? null },
  actions: { kind: "work", from: (r) => r.summary?.records?.length ?? null },
  samples: { kind: "work", from: (r) => r.summary?.samples?.length ?? null },
  closedRetainedBytes: { kind: "work", from: (r) => closedCounts(r)?.retainedBytes ?? null },
  finalRetainedBytes: { kind: "work", from: (r) => finalCounts(r)?.retainedBytes ?? null },
  // Surge-attributed figures: the deltas each surge recorded while it ran and
  // the peak its own ACTIVE window attributed to it, never the account's
  // lifetime counters or total peak.
  surgeRefusalsPressure: { kind: "work", from: (r) => surgeSum(r, (s) => s.refusalsPressure) },
  surgeRefusalsOversized: { kind: "work", from: (r) => surgeSum(r, (s) => s.refusalsOversized) },
  surgeRefusalsActive: {
    kind: "work",
    from: (r) => surgeSum(r, (s) => s.pressure?.refusalsActive),
  },
  surgeActivePeakBytes: {
    kind: "work",
    from: (r) => surgeMax(r, (s) => s.pressure?.active?.attributedPeak),
  },
  wallMs: { kind: "time", from: (r) => r.wallMs ?? null },
  peakRssBytes: {
    kind: "memory",
    from: (r) => r.summary?.process?.peakResidentBytes ?? r.summary?.process?.peakBytes ?? null,
  },
  // The runner is one process and spawns none: its process tree is itself.
  processTreePeakBytes: { kind: "memory", from: (r) => r.summary?.process?.peakBytes ?? null },
};

const isDigest = (v) => typeof v === "string" && /^[0-9a-f]{64}$/.test(v);

function surgeLimitProblems(limits) {
  const at = "soak.surgeLimits";
  const object = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
  if (!object(limits)) return [`${at} must declare bounded and full surge bounds`];
  const problems = [];
  const fields = [
    "modules",
    "props",
    "oversizedProps",
    "inputBytesMax",
    "concurrency",
    "liveArtifactsMax",
    "registeredSourcesMax",
    "snapshotLeasesMax",
    "retainedBytesMax",
  ];
  for (const name of Object.keys(limits))
    if (!["bounded", "full"].includes(name)) problems.push(`${at}: unknown shape ${name}`);
  for (const name of ["bounded", "full"]) {
    const shape = limits[name];
    const where = `${at}.${name}`;
    if (!object(shape)) {
      problems.push(`${where} must be an object`);
      continue;
    }
    for (const field of Object.keys(shape))
      if (!fields.includes(field)) problems.push(`${where}: unknown field ${field}`);
    for (const field of fields)
      if (!Number.isSafeInteger(shape[field]) || shape[field] <= 0)
        problems.push(`${where}.${field} must be a positive safe integer`);
    if (shape.modules !== shape.concurrency)
      problems.push(`${where}.concurrency must cover all requested modules together`);
    for (const field of ["liveArtifactsMax", "registeredSourcesMax", "snapshotLeasesMax"])
      if (shape.modules > shape[field]) problems.push(`${where}.modules exceeds ${field}`);
    const retainedLimit = name === "bounded" ? 64 * 1024 ** 2 : 1024 ** 3;
    if (shape.retainedBytesMax > retainedLimit)
      problems.push(`${where}.retainedBytesMax exceeds the retained-byte policy`);
  }
  return problems;
}

function soakCellProblems(cell, at, modes) {
  const problems = [];
  if (!cell || typeof cell !== "object" || Array.isArray(cell)) return [`${at} is not an object`];
  for (const key of Object.keys(cell))
    if (!SOAK_CELL_FIELDS.includes(key)) problems.push(`${at}: unknown field ${key}`);
  if (!modes.includes(cell.mode))
    problems.push(`${at}: mode ${cell.mode} is not one of the run's modes`);
  if ("benchM3Only" in cell && typeof cell.benchM3Only !== "boolean")
    problems.push(`${at}: benchM3Only must be true or false`);
  if (!stringList(cell.metrics)) return [...problems, `${at}: metrics must be a non-empty list`];
  const kinds = new Set();
  for (const m of cell.metrics)
    if (!SOAK_METRICS[m]) problems.push(`${at}: unknown soak metric ${m}`);
    else kinds.add(SOAK_METRICS[m].kind);
  const benchOnly = cell.benchM3Only === true;
  if (!benchOnly && (kinds.has("time") || kinds.has("memory")))
    problems.push(
      `${at}: a time or memory metric is measured only on ${BENCH_TAG}; mark the cell benchM3Only`,
    );
  if (benchOnly && kinds.has("work"))
    problems.push(`${at}: a benchM3Only cell holds no work metric`);
  return problems;
}

/** Validate a soak manifest; `run.soak` carries what the runner executes. */
export function validateSoakManifest(manifest, file) {
  const problems = [];
  for (const key of Object.keys(manifest))
    if (!SOAK_REQUIRED.includes(key)) problems.push(`unknown field ${key}`);
  for (const key of SOAK_REQUIRED)
    if (!(key in manifest)) problems.push(`missing required field ${key}`);
  if (problems.length) return { problems, run: null };
  const { name, modes, noise, soak, requiredCells } = manifest;
  const stem = file ? file.replace(/^.*[\\/]/, "").replace(/\.json$/, "") : name;
  if (typeof name !== "string" || !/^[a-z0-9][a-z0-9-]*$/.test(name))
    problems.push(`name ${JSON.stringify(name)} must be lower-case letters, digits and hyphens`);
  else if (name !== stem) problems.push(`name ${name} differs from the file name ${stem}.json`);
  if (typeof manifest.description !== "string" || !manifest.description.trim())
    problems.push("description must be a non-empty string");
  const modeList = stringList(modes) && modes.every((m) => SOAK_MODES.includes(m)) ? modes : [];
  if (!modeList.length) problems.push(`modes must be a non-empty list of ${SOAK_MODES.join(", ")}`);
  else if (new Set(modes).size !== modes.length) problems.push("modes lists a mode twice");
  const noiseRefs = typeof noise === "string" ? [noise] : noise;
  if (!stringList(noiseRefs)) problems.push("noise must be a reference or a list of references");
  else {
    const anchors = headingAnchors(join(ROOT, NOISE_DOC));
    for (const ref of noiseRefs) {
      const [doc, anchor] = ref.split("#");
      if (doc !== NOISE_DOC || !anchor || !anchors.has(anchor))
        problems.push(`noise reference ${ref} is not a section of ${NOISE_DOC}`);
    }
  }
  if (!soak || typeof soak !== "object" || Array.isArray(soak))
    return { problems: [...problems, "soak must be an object"], run: null };
  for (const key of Object.keys(soak))
    if (!SOAK_FIELDS.includes(key)) problems.push(`soak: unknown field ${key}`);
  for (const key of SOAK_FIELDS) if (!(key in soak)) problems.push(`soak: missing field ${key}`);
  problems.push(...surgeLimitProblems(soak.surgeLimits));
  if (soak.runner !== "verter-soak") problems.push("soak.runner must be verter-soak");
  for (const key of SOAK_COUNTS)
    if (key in soak && !isInt(soak[key], 1))
      problems.push(`soak.${key} must be a positive integer`);
  if (isInt(soak.sampleEvery, 1) && soak.sampleEvery > 100)
    problems.push("soak.sampleEvery must sample at least every 100 actions");
  for (const mode of modeList) {
    const w = soak.workloads?.[mode];
    if (!w || !isDigest(w.digest) || !isInt(w.actions, 1) || !isInt(w.distinctQueries, 1))
      problems.push(`soak.workloads.${mode} needs a sha-256 digest, actions and distinctQueries`);
    else {
      if (w.actions !== soak.tranches * soak.actionsPerTranche)
        problems.push(`soak.workloads.${mode}.actions is not tranches x actionsPerTranche`);
      if (w.distinctQueries < soak.minDistinctQueries)
        problems.push(
          `soak.workloads.${mode} demands fewer than ${soak.minDistinctQueries} distinct queries`,
        );
    }
    const s = soak.smoke?.[mode];
    if (!s || s.tranches !== 1 || !isDigest(s.digest))
      problems.push(`soak.smoke.${mode} needs tranches 1 and a sha-256 digest`);
  }
  if (soak.controls?.runner !== "verter-soak --controls" || !stringList(soak.controls?.names))
    problems.push('soak.controls needs the runner "verter-soak --controls" and its control names');
  if (soak.binding?.smoke !== "targeted-domain" || soak.binding?.full !== BENCH_TAG)
    problems.push(
      `soak.binding must bind the smoke to targeted-domain and the full run to ${BENCH_TAG}`,
    );
  if (JSON.stringify(soak.build) !== JSON.stringify(SOAK_BUILD))
    problems.push(
      `soak.build must declare the support-feature build ${JSON.stringify(SOAK_BUILD)}`,
    );
  if (!Array.isArray(requiredCells) || !requiredCells.length)
    problems.push("requiredCells must be a non-empty list");
  else {
    const seen = new Set();
    requiredCells.forEach((cell, i) => {
      problems.push(...soakCellProblems(cell, `requiredCells[${i}]`, modeList));
      const key = `${cell?.mode}|${cell?.benchM3Only === true}`;
      if (seen.has(key)) problems.push(`requiredCells[${i}]: duplicate cell ${key}`);
      seen.add(key);
    });
  }
  if (problems.length) return { problems, run: null };
  return {
    problems,
    run: { soak: { modes, tranches: soak.tranches }, sessions: [], requireAllMatched: false },
  };
}

/** One soak cell of the summary (`result` null when the mode did not run). */
export function summarizeSoakCell(cell, result, { dryRun, worker }) {
  const out = { ...cell, status: dryRun ? "not run" : result ? "present" : "missing", metrics: {} };
  for (const m of cell.metrics) {
    const def = SOAK_METRICS[m];
    if (dryRun)
      out.metrics[m] = {
        kind: def.kind,
        status: "not measured",
        reason: "a dry run executes nothing",
      };
    else if (def.kind !== "work" && !worker.benchM3)
      out.metrics[m] = {
        kind: def.kind,
        status: "not measured",
        reason: `${def.kind} is measured only on a worker tagged ${BENCH_TAG}`,
      };
    else {
      const value = result ? def.from(result) : null;
      out.metrics[m] =
        value === null
          ? {
              kind: def.kind,
              status: "unavailable",
              reason: result ? "the run recorded no such figure" : "the mode did not run",
            }
          : { kind: def.kind, status: "measured", values: value };
    }
  }
  return out;
}

/** The evidence summary of a soak run; `results` maps each mode to its run. */
export function buildSoakSummary(loaded, { dryRun, worker, results = {} }) {
  const cells = loaded.manifest.requiredCells.map((cell) =>
    summarizeSoakCell(cell, results[cell.mode] ?? null, { dryRun, worker }),
  );
  const problems = [];
  if (!dryRun) {
    // Correctness and accounting figures are required evidence whatever the
    // runner's verdict says: a required work metric the run did not record
    // fails the summary. Time and memory metrics stay advisory.
    for (const cell of cells)
      for (const [name, metric] of Object.entries(cell.metrics))
        if (metric.kind === "work" && metric.status !== "measured")
          problems.push(
            `cell ${cell.mode}: required work metric ${name} is ${metric.status}: ${metric.reason ?? "no figure"}`,
          );
    const controls = results.controls;
    const frozenControls = loaded.manifest.soak.controls.names;
    const reported = (controls?.controls ?? []).map((c) => c.name);
    if (!controls) problems.push("the negative controls did not run");
    else if (controls.error) problems.push(`negative controls: ${controls.error}`);
    else {
      if (controls.verdict !== "passed")
        problems.push(
          `negative controls failed: ${
            (controls.controls ?? [])
              .filter((c) => !c.bit)
              .map((c) => c.name)
              .join(", ") || "no verdict"
          }`,
        );
      for (const name of frozenControls)
        if (!reported.includes(name)) problems.push(`negative control ${name} did not run`);
    }
    for (const mode of loaded.run.soak.modes) {
      const r = results[mode];
      const frozen = loaded.manifest.soak.workloads[mode].digest;
      if (!r) problems.push(`mode ${mode} did not run`);
      else if (r.error) problems.push(`mode ${mode}: ${r.error}`);
      else {
        // A verdict without the summary it judged proves nothing about the
        // frozen workload, and a summary without its verdict was not judged.
        if (!r.summary) problems.push(`mode ${mode}: the runner wrote no summary`);
        else {
          if (r.summary.manifestDigest !== frozen)
            problems.push(
              `mode ${mode} ran manifest ${r.summary.manifestDigest}, not the frozen ${frozen}`,
            );
          if (r.summary.mode !== mode || r.summary.tranches !== loaded.run.soak.tranches)
            problems.push(`mode ${mode}: the summary is for a different workload`);
        }
        if (r.verdict?.verdict !== "passed")
          problems.push(
            `mode ${mode} failed: ${(r.verdict?.failures ?? []).map((f) => f.kind).join(", ") || "no verdict"}`,
          );
      }
    }
  }
  return {
    schema: SUMMARY_SCHEMA,
    run: {
      name: loaded.manifest.name,
      kind: "soak",
      manifest: relPath(loaded.path),
      manifestSha256: loaded.sha256,
    },
    dryRun,
    worker,
    provenance: dryRun ? null : { candidate: results.candidate ?? null },
    cells,
    validation: dryRun
      ? { verdict: "not run", problems: [] }
      : { verdict: problems.length ? "failed" : "passed", problems },
  };
}

/**
 * Run the soak runner `exe` with `args` into `dir`. Every invocation starts
 * from an empty directory, so an artifact found afterwards was written by this
 * invocation, and a run that ended abnormally (a signal, a spawn failure, a
 * status the runner does not exit with) is an error whatever an earlier run
 * left behind. The runner exits 0 when it passed and 1 when it judged a failure.
 */
export function invokeSoakRunner(exe, args, dir) {
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const started = process.hrtime.bigint();
  const r = spawnSync(exe, [...args, "--out", dir], { cwd: ROOT, stdio: "inherit" });
  const wallMs = Number(process.hrtime.bigint() - started) / 1e6;
  const read = (f) =>
    existsSync(join(dir, f)) ? JSON.parse(readFileSync(join(dir, f), "utf8")) : null;
  const ended =
    r.error || r.signal || (r.status !== 0 && r.status !== 1)
      ? `the runner ended abnormally (${r.error?.message ?? r.signal ?? `status ${r.status}`})`
      : null;
  return { read, wallMs, ended };
}

/** Build the declared runner once, then run every mode of the frozen workload. */
export function defaultSoakRunner(loaded, outDir) {
  const { package: pkg, bin, profile, features } = loaded.manifest.soak.build;
  const build = spawnSync(
    "cargo",
    [
      "build",
      "--profile",
      profile,
      "-p",
      pkg,
      "--bin",
      bin,
      "--features",
      features.join(","),
      "--message-format",
      "json-render-diagnostics",
    ],
    { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  const parse = (line) => {
    try {
      return JSON.parse(line);
    } catch {
      return null;
    }
  };
  const exe = (build.stdout ?? "")
    .split(/\r?\n/)
    .map(parse)
    .find((m) => m?.reason === "compiler-artifact" && m.target?.name === "verter-soak")?.executable;
  const head = spawnSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" });
  const results = { candidate: head.status === 0 ? head.stdout.trim() : null };
  const modes = loaded.run.soak.modes;
  if (build.status !== 0 || !exe) {
    const error = `cargo build of verter-soak exited ${build.status}`;
    results.controls = { error };
    for (const mode of modes) results[mode] = { error };
    return results;
  }
  const invoke = (args, dir) => invokeSoakRunner(exe, args, dir);
  const controls = invoke(["--controls"], join(outDir, "soak", "controls"));
  results.controls = controls.ended
    ? { error: controls.ended }
    : (controls.read("soak-controls.json") ?? { error: "the controls wrote no verdict" });
  for (const mode of modes) {
    if (results.controls.error || results.controls.verdict !== "passed") {
      results[mode] = { error: "the negative controls did not pass; the real run was not started" };
      continue;
    }
    const dir = join(outDir, "soak", mode);
    const run = invoke(["--mode", mode, "--tranches", String(loaded.run.soak.tranches)], dir);
    results[mode] = run.ended
      ? { error: run.ended }
      : {
          verdict: run.read("soak-verdict.json"),
          summary: run.read("soak-summary.json"),
          controls: results.controls,
          wallMs: run.wallMs,
        };
  }
  return results;
}

export const cellKey = (c) => `${c.scenario}/${c.setting}|${c.arm}|${c.mode}|threads=${c.threads}`;

/**
 * Load the manifest `name` from `dir`. Every manifest file in the directory
 * is read for its name: a name two files share (compared case-insensitively,
 * as file systems may) is rejected. Throws with every problem found.
 */
export function loadManifest(name, dir = MANIFEST_DIR) {
  if (!existsSync(dir)) throw new Error(`no manifest directory ${dir}`);
  const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
  const owners = new Map();
  for (const f of files) {
    let declared = f.replace(/\.json$/, "");
    try {
      const parsed = JSON.parse(readFileSync(join(dir, f), "utf8"));
      if (typeof parsed?.name === "string") declared = parsed.name;
    } catch {
      // An unparsable neighbour is reported when it is itself run.
    }
    const key = declared.toLowerCase();
    owners.set(key, [...(owners.get(key) ?? []), f]);
  }
  const file = files.find((f) => f === `${name}.json`);
  if (!file) throw new Error(`no manifest ${name}.json in ${dir}`);
  const path = join(dir, file);
  const bytes = readFileSync(path);
  let manifest;
  try {
    manifest = JSON.parse(bytes.toString("utf8"));
  } catch (err) {
    throw new Error(`manifest ${file} is not valid JSON: ${err.message}`);
  }
  const { problems, run } = validateManifest(manifest, file);
  const sharing = owners.get(String(manifest?.name ?? name).toLowerCase()) ?? [];
  if (sharing.length > 1)
    problems.push(`the name ${manifest.name} is declared by ${sharing.join(", ")}`);
  if (problems.length) throw new Error(`manifest ${file} is invalid:\n  ${problems.join("\n  ")}`);
  return { manifest, run, path, sha256: createHash("sha256").update(bytes).digest("hex") };
}

/** The worker record the evidence job supplies ({ id, tags, ... }), or the unrecorded worker. */
export function readWorker(path) {
  if (!path) return { recorded: false, id: null, tags: [], benchM3: false };
  const record = JSON.parse(readFileSync(path, "utf8"));
  if (
    !record ||
    typeof record.id !== "string" ||
    !record.id ||
    !Array.isArray(record.tags) ||
    !record.tags.every((t) => typeof t === "string")
  )
    throw new Error(`worker record ${path} must hold a string id and a list of string tags`);
  return { ...record, recorded: true, benchM3: record.tags.includes(BENCH_TAG) };
}

/** Problems where an executed run differs from the manifest's invocation. */
export function invocationProblems(results, loaded) {
  const { manifest, run } = loaded;
  const opts = results?.meta?.options ?? {};
  const problems = [];
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  if (opts.tier !== manifest.tier)
    problems.push(`the run's tier ${opts.tier} is not the manifest's ${manifest.tier}`);
  if (!same(opts.arms, manifest.arms))
    problems.push(`the run's arms ${opts.arms} are not the manifest's ${manifest.arms}`);
  if (opts.settings !== (run.settings.length === 1 ? "strict" : "all"))
    problems.push(`the run's settings ${opts.settings} are not the manifest's ${run.settings}`);
  for (const key of ["repeat", "warmup", "warmRepeats"])
    if (key in manifest && opts[key] !== manifest[key])
      problems.push(`the run's ${key} ${opts[key]} is not the manifest's ${manifest[key]}`);
  const ran = [...new Set(Object.values(results?.meta?.scenarios ?? {}).map((s) => s.id))].sort();
  if (!same(ran, [...run.scenarios].sort()))
    problems.push("the run's scenarios are not the manifest's");
  // Sessions are checked only where the manifest lists them: a tier run
  // also carries its tier's sessions, which the manifest leaves unnamed.
  if (run.sessions.length) {
    const ranSessions = Object.keys(results?.meta?.sessions ?? {}).sort();
    if (!same(ranSessions, [...run.sessions].sort()))
      problems.push("the run's sessions are not the manifest's");
  }
  return problems;
}

/**
 * One listed session of the summary, read from the harness's session
 * summary: its Verter answer class and, for a growth session, its retention
 * proof. A retention proof's figures are deterministic retention-account
 * bytes and object counts, so it is read on every worker.
 */
export function summarizeSession(id, harnessSummary, { dryRun }) {
  const cell = harnessSummary?.sessions?.cells?.find((c) => c.key === id);
  if (dryRun) return { session: id, status: "not run", answerClass: null, retentionProof: null };
  if (!cell) return { session: id, status: "missing", answerClass: null, retentionProof: null };
  return {
    session: id,
    status: "present",
    answerClass: cell.arms?.verter?.class ?? null,
    retentionProof: cell.retentionProof ?? null,
  };
}

/** One required cell of the summary, read from the harness's summary (null when not executed). */
export function summarizeCell(cell, harnessSummary, { dryRun, worker }) {
  const defs = metricsFor(cell.arm, cell.mode);
  const out = { ...cell, metrics: {} };
  const harnessCell = harnessSummary?.cells?.find(
    (c) => c.key === `${cell.scenario}/${cell.setting}`,
  );
  const arm = harnessCell?.arms?.[cell.arm] ?? null;
  if (dryRun) {
    out.status = "not run";
    out.answerClass = null;
  } else if (!arm) {
    out.status = "missing";
    out.answerClass = null;
  } else {
    out.status = "present";
    out.answerClass = ARMS[cell.arm].kind === "cli" ? arm.status : arm.class;
    if (ARMS[cell.arm].kind === "cli") out.codes = arm.codes ?? null;
    else {
      out.repetitionsDiffer = arm.repetitionsDiffer ?? false;
      out.answerDigest = arm.answerDigest ?? null;
    }
  }
  for (const m of cell.metrics) {
    const def = defs[m];
    const measuredHere = def.kind === "work" || worker.benchM3;
    if (dryRun)
      out.metrics[m] = {
        kind: def.kind,
        status: "not measured",
        reason: "a dry run executes nothing",
      };
    else if (!measuredHere)
      out.metrics[m] = {
        kind: def.kind,
        status: "not measured",
        reason: `${def.kind} is measured only on a worker tagged ${BENCH_TAG}`,
      };
    else {
      const value = arm ? def.from(arm, m) : null;
      out.metrics[m] =
        value === null
          ? {
              kind: def.kind,
              status: "unavailable",
              reason: !arm
                ? "the run has no such cell"
                : (arm.memoryUnavailable ?? "no completed measured invocation carries it"),
            }
          : { kind: def.kind, status: "measured", values: value };
    }
  }
  return out;
}

const relPath = (p) => relative(ROOT, p).split("\\").join("/");

function dryPrerequisites() {
  const out = {};
  out.supervisor = existsSync(join(ROOT, "crates", "verter_supervise", "Cargo.toml"))
    ? { status: "met", detail: "crates/verter_supervise (built by the run)" }
    : { status: "unavailable", reason: "no crates/verter_supervise in this checkout" };
  out.containment = {
    status: "unavailable",
    reason: "a dry run starts no supervisor; the run records its backend",
  };
  out.toolchain = existsSync(join(ROOT, "rust-toolchain.toml"))
    ? { status: "met", detail: "rust-toolchain.toml" }
    : { status: "unavailable", reason: "no rust-toolchain.toml" };
  try {
    const ts = resolveTypeScript(ROOT);
    out.typescript = { status: "met", detail: `${ts.versionText} (${ts.platformPackage})` };
  } catch (err) {
    out.typescript = { status: "unavailable", reason: String(err.message ?? err) };
  }
  return out;
}

function runPrerequisites(results, harnessFailure) {
  const meta = results?.meta;
  const missing = (what) => ({
    status: "unavailable",
    reason: harnessFailure ?? `the run's results record no ${what}`,
  });
  const backends = [
    ...new Set(
      (results?.invocations ?? [])
        .filter((i) => i.supervisor)
        .map((i) => `${i.supervisor.backend}/${i.supervisor.containment}`),
    ),
  ];
  return {
    supervisor: meta?.binaries?.supervisor
      ? {
          status: "met",
          detail:
            `${meta.binaries.supervisor.origin ?? "supervisor"} ${meta.binaries.supervisor.sha256 ?? ""}`.trim(),
        }
      : missing("supervisor"),
    containment: backends.length
      ? backends.every((b) => /\/(hard|sampled)$/.test(b))
        ? { status: "met", detail: backends.join(", ") }
        : { status: "unavailable", reason: `uncontained invocations: ${backends.join(", ")}` }
      : missing("containment backend"),
    toolchain: meta?.build
      ? { status: "met", detail: meta.build.cargo ?? "release probe build" }
      : missing("build"),
    typescript: meta?.typescript
      ? {
          status: "met",
          detail: `${meta.typescript.versionText} (${meta.typescript.platformPackage})`,
        }
      : missing("TypeScript package"),
  };
}

function provenanceOf(results) {
  const meta = results?.meta;
  if (!meta) return null;
  return {
    tree: meta.tree ?? null,
    buildInputs: meta.buildInputs ?? null,
    harness: meta.harness ?? null,
    host: meta.host ?? null,
    binaries: meta.binaries ?? null,
    typescript: meta.typescript ?? null,
    environment: meta.environment ?? null,
    tuning: meta.tuning ?? null,
    expected: meta.expected ?? null,
    containment: [
      ...new Set(
        (results.invocations ?? [])
          .filter((i) => i.supervisor)
          .map((i) => i.supervisor.containment),
      ),
    ],
  };
}

/** Build the evidence summary of a run (results null for a dry run or a run that produced none). */
export function buildSummary(loaded, { dryRun, worker, results, harness, validatorProblems }) {
  const harnessFailure =
    harness && harness.status !== 0 && !results
      ? `the harness exited ${harness.status} without results`
      : null;
  const cells = loaded.manifest.requiredCells.map((cell) =>
    summarizeCell(cell, results?.summary, { dryRun, worker }),
  );
  const sessions = (loaded.run.sessions ?? []).map((id) =>
    summarizeSession(id, results?.summary, { dryRun }),
  );
  const problems = [];
  if (!dryRun) {
    if (harnessFailure) problems.push(harnessFailure);
    else if (harness && harness.status !== 0)
      problems.push(`the harness exited ${harness.status} (its validation failed)`);
    if (results) problems.push(...invocationProblems(results, loaded));
    problems.push(...(validatorProblems ?? []));
    for (const cell of cells)
      if (cell.status === "missing") problems.push(`required cell ${cellKey(cell)} is missing`);
    for (const session of sessions) {
      if (session.status === "missing") problems.push(`session ${session.session} is missing`);
      const proof = session.retentionProof;
      const proved = allSessions().find((s) => s.id === session.session)?.retentionProof;
      if (session.status === "present" && proved) {
        // The proof compares what answering retains, so every answer of the
        // growth and of its baseline must be the constructed one.
        for (const id of [session.session, proved.baseline]) {
          const answered = sessions.find((s) => s.session === id)?.answerClass;
          if (answered !== "matched")
            problems.push(
              `session ${session.session}: the retention proof needs ${id} to answer every demand, its class is ${answered ?? "missing"}`,
            );
        }
        if (!proof || proof.verdict === "n/a")
          problems.push(
            `session ${session.session}: the retention proof has no completed pair to compare`,
          );
        else if (proof.verdict !== "passed")
          problems.push(
            `session ${session.session}: retained growth beyond the answers against ${proof.baseline}: ${proof.problems.join("; ")}`,
          );
      }
    }
  }
  return {
    schema: SUMMARY_SCHEMA,
    run: {
      name: loaded.manifest.name,
      manifest: relPath(loaded.path),
      manifestSha256: loaded.sha256,
      requireAllMatched: loaded.run.requireAllMatched,
    },
    dryRun,
    worker,
    provenance: dryRun ? { harness: harnessFingerprint(ROOT) } : provenanceOf(results),
    prerequisites: dryRun ? dryPrerequisites() : runPrerequisites(results, harnessFailure),
    cells,
    sessions,
    answerClasses: results?.summary?.verterClassCounts ?? null,
    validation: dryRun
      ? { verdict: "not run", problems: [] }
      : { verdict: problems.length ? "failed" : "passed", problems },
  };
}

function defaultHarness(args) {
  const r = spawnSync(process.execPath, [join(HERE, "semantic-perf.mjs"), ...args], {
    cwd: ROOT,
    stdio: "inherit",
  });
  return { status: r.status ?? 2 };
}

/**
 * The harness's own validator over the written results (its FAIL lines are
 * the problems). `requireAllMatched` forwards the manifest's policy as
 * validate.mjs's `--require-all-matched`, so a Verter answer that is not
 * matched (wrong, partial, refused or killed) fails the run instead of being
 * reported as a finding. `script` replaces validate.mjs (self-tests).
 */
export function defaultValidator(
  resultsPath,
  { requireAllMatched = false, script = join(HERE, "semantic-perf", "validate.mjs") } = {},
) {
  const r = spawnSync(
    process.execPath,
    [script, resultsPath, ...(requireAllMatched ? ["--require-all-matched"] : [])],
    {
      cwd: ROOT,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
    },
  );
  const fails = (r.stdout ?? "")
    .split(/\r?\n/)
    .filter((l) => l.startsWith("FAIL: "))
    .map((l) => l.slice(6));
  if (r.status !== 0 && !fails.length)
    fails.push(`validate.mjs exited ${r.status}: ${(r.stderr ?? "").trim()}`);
  return fails;
}

export const USAGE = `usage: node scripts/benchmark/evidence-run.mjs --run <name> [options]

  --run <name>            the manifest scripts/benchmark/evidence-runs/<name>.json
  --dry-run               validate the manifest and write the summary skeleton; run nothing
  --candidate <identity>  required review candidate identity for formatter evidence
  --corpus <id>           required performance-catalog fixture id for formatter evidence
  --out <dir>             output directory (default: target/evidence-runs/<name>/<timestamp>)
  --worker <file>         the worker record the evidence job supplies: JSON { "id": "...", "tags": [...] };
                          without it the run is treated as not ${BENCH_TAG}
  --allow-sampled         passed to the harness: consent to a sampled memory cap (macOS)
  --supervisor <path>     passed to the harness: the verter-supervise executable
  --manifest-dir <dir>    read manifests from <dir> instead (self-tests)
  --help`;

function parseArgs(argv) {
  const opts = {
    run: null,
    dryRun: false,
    out: null,
    worker: null,
    passThrough: [],
    manifestDir: MANIFEST_DIR,
    candidate: null,
    corpus: null,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    switch (a) {
      case "--run":
        opts.run = next();
        break;
      case "--dry-run":
        opts.dryRun = true;
        break;
      case "--candidate":
        opts.candidate = next();
        break;
      case "--corpus":
        opts.corpus = next();
        break;
      case "--out":
        opts.out = resolve(next());
        break;
      case "--worker":
        opts.worker = resolve(next());
        break;
      case "--allow-sampled":
        opts.passThrough.push(a);
        break;
      case "--supervisor":
        opts.passThrough.push(a, resolve(next()));
        break;
      case "--manifest-dir":
        opts.manifestDir = resolve(next());
        break;
      case "--help":
        opts.help = true;
        break;
      default:
        throw new Error(`unknown option ${a}\n${USAGE}`);
    }
  }
  if (!opts.help && !opts.run) throw new Error(`--run <name> is required\n${USAGE}`);
  return opts;
}

/**
 * Run the CLI. Exit codes: 0 the run (or dry run) passed, 1 it ran and
 * failed, 2 a usage or manifest error. `deps` replaces the harness and its
 * validator (self-tests).
 */
export async function main(argv, deps = {}) {
  const opts = parseArgs(argv);
  if (opts.help) {
    console.log(USAGE);
    return 0;
  }
  const loaded = loadManifest(opts.run, opts.manifestDir);
  if (!loaded.run.formatter && (opts.candidate !== null || opts.corpus !== null))
    throw Error("--candidate and --corpus are formatter-only parameters");
  const worker = readWorker(opts.worker);
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const outDir = opts.out ?? join(ROOT, "target", "evidence-runs", loaded.manifest.name, stamp);
  mkdirSync(outDir, { recursive: true });
  let summary;
  if (loaded.run.formatter) {
    summary = buildFormatterSummary(loaded, { ...opts, worker });
  } else if (loaded.run.soak) {
    const runSoak = deps.runSoak ?? defaultSoakRunner;
    summary = opts.dryRun
      ? buildSoakSummary(loaded, { dryRun: true, worker })
      : buildSoakSummary(loaded, { dryRun: false, worker, results: runSoak(loaded, outDir) });
  } else if (opts.dryRun) summary = buildSummary(loaded, { dryRun: true, worker });
  else {
    const harnessDir = join(outDir, "harness");
    const runHarness = deps.runHarness ?? defaultHarness;
    const validate = deps.validate ?? defaultValidator;
    const harness = runHarness([
      ...loaded.run.harnessArgs,
      "--out",
      harnessDir,
      ...opts.passThrough,
    ]);
    const resultsPath = join(harnessDir, "results.json");
    const results = existsSync(resultsPath) ? JSON.parse(readFileSync(resultsPath, "utf8")) : null;
    const validatorProblems = results
      ? validate(resultsPath, { requireAllMatched: loaded.run.requireAllMatched })
      : [];
    summary = buildSummary(loaded, { dryRun: false, worker, results, harness, validatorProblems });
  }
  const summaryPath = join(outDir, SUMMARY_FILE);
  writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
  console.log(`evidence-run: ${loaded.manifest.name} summary ${summaryPath}`);
  console.log(`evidence-run: validation ${summary.validation.verdict}`);
  for (const p of summary.validation.problems.slice(0, 40)) console.log(`  - ${p}`);
  return summary.validation.verdict === "failed" ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (err) => {
      console.error(`evidence-run: ${err?.message ?? err}`);
      process.exit(2);
    },
  );
}
