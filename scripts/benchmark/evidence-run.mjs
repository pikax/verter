#!/usr/bin/env node
// Evidence runs: named, frozen semantic-benchmark invocations. A manifest
// (scripts/benchmark/evidence-runs/<name>.json) fixes what runs and which
// cells must come back; this runner validates it, drives semantic-perf.mjs
// with the manifest's options, re-validates the result with the harness's
// own validate.mjs and writes one machine-readable summary. It adds no
// second harness, classifier or validator.
//
//   node scripts/benchmark/evidence-run.mjs --run <name> --dry-run
//   node scripts/benchmark/evidence-run.mjs --run <name> [--worker <record.json>]
//
// See docs/contributing/semantic-benchmark.md#evidence-runs.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { ARMS } from "./semantic-perf/analyze.mjs";
import { harnessFingerprint, resolveTypeScript } from "./semantic-perf/provenance.mjs";
import { scenariosForTier, selectScenarios, SETTINGS, TIERS } from "./semantic-perf/scenarios.mjs";

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
const OPTIONAL_FIELDS = ["scenarios", "settings", "repeat", "warmup", "warmRepeats"];
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
          scenarios.push(...selectScenarios([prefix]).filter((s) => !scenarios.includes(s)));
        } catch {
          problems.push(`scenario ${prefix} selects no scenario of the catalog`);
        }
      }
  } else if (TIERS.includes(tier)) scenarios = scenariosForTier(tier);
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

  const run = {
    scenarios: scenarios.map((s) => s.id),
    settings,
    harnessArgs: [
      "--tier",
      tier,
      ...("scenarios" in manifest ? ["--only", manifest.scenarios.join(",")] : []),
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
  return problems;
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
  const problems = [];
  if (!dryRun) {
    if (harnessFailure) problems.push(harnessFailure);
    else if (harness && harness.status !== 0)
      problems.push(`the harness exited ${harness.status} (its validation failed)`);
    if (results) problems.push(...invocationProblems(results, loaded));
    problems.push(...(validatorProblems ?? []));
    for (const cell of cells)
      if (cell.status === "missing") problems.push(`required cell ${cellKey(cell)} is missing`);
  }
  return {
    schema: SUMMARY_SCHEMA,
    run: {
      name: loaded.manifest.name,
      manifest: relPath(loaded.path),
      manifestSha256: loaded.sha256,
    },
    dryRun,
    worker,
    provenance: dryRun ? { harness: harnessFingerprint(ROOT) } : provenanceOf(results),
    prerequisites: dryRun ? dryPrerequisites() : runPrerequisites(results, harnessFailure),
    cells,
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

/** The harness's own validator over the written results (its FAIL lines are the problems). */
function defaultValidator(resultsPath) {
  const r = spawnSync(
    process.execPath,
    [join(HERE, "semantic-perf", "validate.mjs"), resultsPath],
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
  const worker = readWorker(opts.worker);
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const outDir = opts.out ?? join(ROOT, "target", "evidence-runs", loaded.manifest.name, stamp);
  mkdirSync(outDir, { recursive: true });
  let summary;
  if (opts.dryRun) summary = buildSummary(loaded, { dryRun: true, worker });
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
    const validatorProblems = results ? validate(resultsPath) : [];
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
