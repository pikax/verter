// The benchmark run: materialise every (scenario, setting), schedule every
// arm's warmup and measured invocations in counterbalanced order, run each
// under the supervisor, keep every record, then summarise, validate and
// report. See docs/contributing/semantic-benchmark.md.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { ARMS, compactProbeRecord, DEFAULT_ARMS } from "./analyze.mjs";
import {
  BUILD_INPUTS,
  buildProblems,
  harnessFingerprint,
  buildVerterProbes,
  hostInfo,
  pinBinary,
  probeIdentity,
  resolveTypeScript,
  sha256File,
  sha256Text,
  sourceTree,
} from "./provenance.mjs";
import { renderMarkdown } from "./report.mjs";
import { cliSource, SETTINGS, selectScenarios, tsconfigText } from "./scenarios.mjs";
import { summarize } from "./summary.mjs";
import { resolveSupervisor, runSupervised } from "./supervisor.mjs";
import { validateRun } from "./validate.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
export const ROOT = resolve(HERE, "..", "..", "..");
export const LIB_FILE = join(HERE, "lib", "bench-globals.d.ts");
export const EXPECTED_FILE = join(HERE, "expected.json");
export const RESULTS_SCHEMA = 1;

export const USAGE = `usage: node scripts/benchmark/semantic-perf.mjs [options]

  --out <dir>             output directory (default: target/semantic-perf/<timestamp>)
  --only <a,b>            only scenarios whose id is or starts with one of these
  --settings <s>          strict (default) | all  (the four strictNullChecks x noImplicitAny settings)
  --arms <a,b>            arms to run (default: ${DEFAULT_ARMS.join(",")})
  --repeat <n>            measured invocations per arm (default 4; at least 2, even keeps the order balanced)
  --warmup <n>            warmup invocations per arm, run and validated but not measured (default 1)
  --warm-repeats <n>      in-process repeats of the probe after its cold request (default 5)
  --mem-mb <n>            the engine memory budget, equal for both tools (default 8192): an engine whose
                          own peak exceeds it counts as exhausting it
  --infra-mb <n>          containment allowance above the budget for the process tree's other members
                          (tsc's node client, the statistics reader) (default 1024)
  --timeout-ms <n>        per-invocation deadline (default 300000)
  --allow-tuning          run although runtime or build tuning variables (GOGC, GOMAXPROCS, NODE_OPTIONS,
                          RUSTFLAGS, …) are set; the report is labelled tuned
  --supervisor <path>     verter-supervise executable (default: build crates/verter_supervise)
  --allow-sampled         consent to a sampled (not kernel-enforced) memory cap; required on macOS
  --typescript-from <dir> directory whose node_modules resolves typescript@7.0.2 (default: repository root)
  --lib-mode <m>          how Verter reads the library: root-file (default, as tsc does) | ambient
  --no-skip-after-kill    re-run every invocation of an arm whose warmup was killed at the memory cap
                          (by default the rest are recorded as skipped: a memory kill is deterministic)
  --help`;

function parseArgs(argv) {
  const opts = {
    out: null,
    only: [],
    settings: "strict",
    arms: DEFAULT_ARMS,
    repeat: 4,
    warmup: 1,
    warmRepeats: 5,
    memMb: 8192,
    infraMb: 1024,
    timeoutMs: 300_000,
    allowTuning: false,
    supervisor: null,
    allowSampled: false,
    typescriptFrom: ROOT,
    libMode: "root-file",
    skipAfterKill: true,
  };
  const positive = (name, value, min = 1) => {
    const n = Number(value);
    if (!Number.isInteger(n) || n < min) throw new Error(`${name} must be an integer >= ${min}, got ${value}`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    switch (a) {
      case "--out":
        opts.out = resolve(next());
        break;
      case "--only":
        opts.only = next().split(",").filter(Boolean);
        break;
      case "--settings": {
        const v = next();
        if (!["strict", "all"].includes(v)) throw new Error(`--settings must be strict or all`);
        opts.settings = v;
        break;
      }
      case "--arms": {
        const arms = next().split(",").filter(Boolean);
        for (const arm of arms) if (!ARMS[arm]) throw new Error(`unknown arm ${arm}; arms: ${DEFAULT_ARMS.join(", ")}`);
        opts.arms = arms;
        break;
      }
      case "--repeat":
        opts.repeat = positive(a, next(), 2);
        if (opts.repeat % 2) throw new Error("--repeat must be even: each arm runs first in exactly half the measured rounds");
        break;
      case "--warmup":
        opts.warmup = positive(a, next(), 0);
        break;
      case "--warm-repeats":
        opts.warmRepeats = positive(a, next(), 1);
        break;
      case "--mem-mb":
        opts.memMb = positive(a, next());
        break;
      case "--infra-mb":
        opts.infraMb = positive(a, next());
        break;
      case "--timeout-ms":
        opts.timeoutMs = positive(a, next());
        break;
      case "--allow-tuning":
        opts.allowTuning = true;
        break;
      case "--supervisor":
        opts.supervisor = resolve(next());
        break;
      case "--allow-sampled":
        opts.allowSampled = true;
        break;
      case "--typescript-from":
        opts.typescriptFrom = resolve(next());
        break;
      case "--lib-mode": {
        const v = next();
        if (!["root-file", "ambient"].includes(v)) throw new Error(`--lib-mode must be root-file or ambient`);
        opts.libMode = v;
        break;
      }
      case "--no-skip-after-kill":
        opts.skipAfterKill = false;
        break;
      case "--help":
        opts.help = true;
        break;
      default:
        throw new Error(`unknown option ${a}\n${USAGE}`);
    }
  }
  return opts;
}

function loadExpected() {
  if (!existsSync(EXPECTED_FILE)) throw new Error(`the measured reference ${EXPECTED_FILE} is missing; run measure-expected.mjs`);
  return JSON.parse(readFileSync(EXPECTED_FILE, "utf8"));
}

/**
 * The counterbalanced schedule: warmup rounds, then measured rounds. The
 * cell order reverses on alternate rounds; each cell's arm order alternates
 * from one round to the next (by the cell's own catalog index, so the two
 * reversals never cancel), so over an even number of measured rounds every
 * pair of arms runs in each order equally often, in every cell.
 */
export function schedule(scenarioKeys, arms, repeat, warmup) {
  const out = [];
  const rounds = [...Array(warmup).keys()].map((i) => ({ rep: i, warmup: true }));
  for (let i = 0; i < repeat; i++) rounds.push({ rep: i, warmup: false });
  rounds.forEach((round, roundIndex) => {
    const keys = roundIndex % 2 === 0 ? scenarioKeys : [...scenarioKeys].reverse();
    for (const key of keys) {
      const cellIndex = scenarioKeys.indexOf(key);
      const order = (roundIndex + cellIndex) % 2 === 0 ? arms : [...arms].reverse();
      for (const arm of order) out.push({ key, arm, rep: round.rep, warmup: round.warmup });
    }
  });
  return out;
}

/**
 * Problems with a plan's balance: in every cell, over the measured rounds,
 * every pair of arms must run in each order equally often.
 */
export function scheduleBalanceProblems(plan, arms) {
  const problems = [];
  const byCellRound = new Map();
  for (const step of plan) {
    if (step.warmup) continue;
    const k = `${step.key}|${step.rep}`;
    if (!byCellRound.has(k)) byCellRound.set(k, []);
    byCellRound.get(k).push(step.arm);
  }
  const counts = new Map();
  for (const [k, order] of byCellRound) {
    const key = k.slice(0, k.lastIndexOf("|"));
    for (let i = 0; i < arms.length; i++) {
      for (let j = i + 1; j < arms.length; j++) {
        const a = arms[i];
        const b = arms[j];
        const c = `${key}|${a}|${b}`;
        if (!counts.has(c)) counts.set(c, 0);
        if (order.indexOf(a) < order.indexOf(b)) counts.set(c, counts.get(c) + 1);
        else counts.set(c, counts.get(c) - 1);
      }
    }
  }
  for (const [c, n] of counts) if (n !== 0) problems.push(`unbalanced order ${c} (${n > 0 ? "first" : "second"} by ${Math.abs(n)})`);
  return problems;
}

/** Runtime and build variables that tune either tool away from its shipped defaults. */
export const TUNING_VARIABLES = [
  "GOGC",
  "GOMEMLIMIT",
  "GOMAXPROCS",
  "GODEBUG",
  "GOTRACEBACK",
  "NODE_OPTIONS",
  "UV_THREADPOOL_SIZE",
  "RUSTFLAGS",
  "CARGO_ENCODED_RUSTFLAGS",
  "CARGO_BUILD_RUSTFLAGS",
  "CARGO_BUILD_TARGET",
  "RAYON_NUM_THREADS",
  "MIMALLOC_OPTIONS",
  "MALLOC_CONF",
];

/** The tuning variables set in `env` (and every CARGO_PROFILE_* / VERTER_* one). */
export function tuningEnvironment(env) {
  return Object.fromEntries(
    Object.entries(env)
      .filter(([k]) => TUNING_VARIABLES.includes(k) || /^CARGO_PROFILE_/.test(k) || /^VERTER_/.test(k) || /^CARGO_TARGET_.*_RUSTFLAGS$/.test(k))
      .sort(([a], [b]) => a.localeCompare(b)),
  );
}

/** Rust's and Node's names for one architecture. */
export function sameArchitecture(rustArch, nodeArch) {
  const map = { x86_64: "x64", aarch64: "arm64", x86: "ia32", arm: "arm" };
  return (map[rustArch] ?? rustArch) === nodeArch;
}

function materialize(outDir, scenario, setting, opts, libText) {
  const dir = join(outDir, "scenarios", scenario.id, setting.id);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "lib.bench.d.ts"), libText);
  writeFileSync(join(dir, "scenario.ts"), scenario.source);
  writeFileSync(join(dir, "tsconfig.json"), tsconfigText(setting));
  const cliDir = join(dir, "cli");
  mkdirSync(cliDir, { recursive: true });
  writeFileSync(join(cliDir, "lib.bench.d.ts"), libText);
  writeFileSync(join(cliDir, "scenario.ts"), cliSource(scenario));
  writeFileSync(join(cliDir, "tsconfig.json"), tsconfigText(setting));
  const baseJob = {
    schema: 1,
    dir,
    tsconfig: "tsconfig.json",
    lib: "lib.bench.d.ts",
    libMode: opts.libMode,
    scenario: "scenario.ts",
    initAlias: "__BenchInit",
    probes: ["__Probe"],
    warmRepeats: opts.warmRepeats,
  };
  return {
    dir,
    inputs: {
      "lib.bench.d.ts": sha256Text(libText),
      "scenario.ts": sha256Text(scenario.source),
      "tsconfig.json": sha256Text(tsconfigText(setting)),
      "cli/scenario.ts": sha256Text(cliSource(scenario)),
    },
    baseJob,
  };
}

function writeJob(path, job) {
  writeFileSync(path, JSON.stringify(job, null, 2));
  return path;
}

function commandFor(arm, ctx, runBase) {
  const { binaries, typescript, scenarioDir, baseJob } = ctx;
  const probeOut = `${runBase}.probe.json`;
  switch (arm) {
    case "verter":
      return {
        argv: [binaries.probe.pinned, "run", "--job", writeJob(`${runBase}.job.json`, { ...baseJob, observability: false }), "--out", probeOut],
        probeOut,
      };
    case "verter-obs":
      return {
        argv: [binaries.probe.pinned, "run", "--job", writeJob(`${runBase}.job.json`, { ...baseJob, observability: true }), "--out", probeOut],
        probeOut,
      };
    case "verter-counted":
      return {
        argv: [binaries.counted.pinned, "run", "--job", writeJob(`${runBase}.job.json`, { ...baseJob, observability: false }), "--out", probeOut],
        probeOut,
      };
    case "tsc-api": {
      const { libMode: _unused, ...shared } = baseJob;
      const job = { ...shared, tsPackageDir: typescript.packageDir, tscExe: typescript.exe, statsExe: binaries.probe.pinned };
      return {
        argv: [process.execPath, join(HERE, "tsc-probe.mjs"), "--job", writeJob(`${runBase}.job.json`, job), "--out", probeOut],
        probeOut,
      };
    }
    case "tsc-cli":
      return { argv: [typescript.exe, "-p", join(scenarioDir, "cli", "tsconfig.json"), "--extendedDiagnostics"], probeOut: null };
    case "tsc-cli-1":
      return {
        argv: [typescript.exe, "-p", join(scenarioDir, "cli", "tsconfig.json"), "--extendedDiagnostics", "--singleThreaded"],
        probeOut: null,
      };
    default:
      throw new Error(`no command for arm ${arm}`);
  }
}

export async function main(argv) {
  const opts = parseArgs(argv);
  if (opts.help) {
    console.log(USAGE);
    return 0;
  }
  if (process.platform === "darwin" && !opts.allowSampled) {
    throw new Error(
      "macOS has no kernel-enforced process-tree memory cap: the supervisor samples. Re-run with --allow-sampled to consent; every record will say `containment: sampled`.",
    );
  }
  const tuning = tuningEnvironment(process.env);
  if (Object.keys(tuning).length && !opts.allowTuning) {
    throw new Error(
      `tuning variables are set (${Object.keys(tuning).join(", ")}): the benchmark measures both tools as shipped. Unset them, or pass --allow-tuning to run a labelled tuned benchmark.`,
    );
  }
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const outDir = opts.out ?? join(ROOT, "target", "semantic-perf", stamp);
  mkdirSync(outDir, { recursive: true });
  const log = (line) => console.log(line);

  log(`semantic-perf: output ${outDir}`);
  const tree = sourceTree(ROOT);
  const buildInputs = sourceTree(ROOT, BUILD_INPUTS);
  const harness = harnessFingerprint(ROOT);
  const typescript = resolveTypeScript(opts.typescriptFrom);
  log(`semantic-perf: ${typescript.versionText} at ${typescript.exe}`);
  log("semantic-perf: building the release Verter probe binaries");
  const build = buildVerterProbes(ROOT);
  const problems = buildProblems(build);
  if (problems.length) throw new Error(`the Verter probe is not a production build:\n  ${problems.join("\n  ")}`);
  const binDir = join(outDir, "bin");
  const binaries = {
    probe: pinBinary(build.executables.semantic_perf_probe, binDir, "semantic_perf_probe"),
    counted: pinBinary(build.executables.semantic_perf_probe_counted, binDir, "semantic_perf_probe_counted"),
  };
  binaries.probe.identity = probeIdentity(binaries.probe.pinned);
  binaries.counted.identity = probeIdentity(binaries.counted.pinned);
  for (const [name, bin] of [
    ["the Verter probe", binaries.probe],
    ["the counted probe", binaries.counted],
  ]) {
    if (!sameArchitecture(bin.identity.targetArch, process.arch) || !typescript.platformPackage.endsWith(`-${process.arch}`)) {
      throw new Error(`${name} is built for ${bin.identity.targetArch}, node runs ${process.arch} and tsc is ${typescript.platformPackage}: the tools must run natively on one architecture`);
    }
  }
  const supervisorSource = resolveSupervisor(ROOT, opts.supervisor);
  binaries.supervisor = { ...pinBinary(supervisorSource.path, binDir, "verter-supervise"), origin: supervisorSource.origin };
  // The probe's build inputs, read again after the build: a change between
  // the two reads means the binary's source is unknown.
  const buildInputsAfterBuild = sourceTree(ROOT, BUILD_INPUTS);

  const expected = loadExpected();
  const libText = readFileSync(LIB_FILE, "utf8");
  const scenarios = selectScenarios(opts.only);
  const settings = opts.settings === "all" ? SETTINGS : SETTINGS.filter((s) => s.id === "strict");
  const cells = new Map();
  for (const scenario of scenarios) {
    for (const setting of settings) {
      const key = `${scenario.id}/${setting.id}`;
      cells.set(key, { scenario, setting, ...materialize(outDir, scenario, setting, opts, libText) });
    }
  }

  const plan = schedule([...cells.keys()], opts.arms, opts.repeat, opts.warmup);
  const invocations = [];
  const started = Date.now();
  // (cell, arm) pairs whose warmup was killed at the memory cap: a memory
  // kill is deterministic, so the remaining invocations are recorded as
  // skipped instead of driving the machine to the cap again.
  const memoryKilled = new Map();
  for (const [index, step] of plan.entries()) {
    const cell = cells.get(step.key);
    const cellArm = `${step.key}|${step.arm}`;
    if (opts.skipAfterKill && memoryKilled.has(cellArm)) {
      invocations.push({
        index,
        scenario: cell.scenario.id,
        setting: cell.setting.id,
        arm: step.arm,
        rep: step.rep,
        warmup: step.warmup,
        skipped: { after: memoryKilled.get(cellArm), reason: "a warmup of this scenario and arm was killed at the memory cap" },
      });
      log(`[${index + 1}/${plan.length}] ${step.key} ${step.arm} ${step.warmup ? "warmup" : "rep"} ${step.rep}: skipped (warmup killed at the memory cap)`);
      continue;
    }
    const runDir = join(outDir, "runs", cell.scenario.id, cell.setting.id, step.arm);
    mkdirSync(runDir, { recursive: true });
    const runBase = join(runDir, `${step.warmup ? "warmup" : "rep"}-${step.rep}`);
    const { argv: command, probeOut } = commandFor(step.arm, { binaries, typescript, scenarioDir: cell.dir, baseJob: cell.baseJob }, runBase);
    const supOut = `${runBase}.sup.json`;
    const t0 = Date.now();
    const result = await runSupervised(binaries.supervisor.pinned, {
      // The engine budget plus the allowance for the tree's other members;
      // the engine's own peak is held to the budget when classifying.
      memMb: opts.memMb + opts.infraMb,
      timeoutMs: opts.timeoutMs,
      out: supOut,
      cwd: cell.dir,
      argv: command,
      allowSampled: opts.allowSampled,
    });
    let probe = null;
    let probeReadError = null;
    let phase = null;
    if (probeOut) {
      try {
        phase = readFileSync(`${probeOut}.phase`, "utf8").trim();
      } catch {
        phase = null;
      }
      try {
        probe = compactProbeRecord(JSON.parse(readFileSync(probeOut, "utf8")));
      } catch (err) {
        probeReadError = String(err.message ?? err);
      }
    }
    let cliStdout = null;
    if (!probeOut && result.record?.stdoutPath) {
      try {
        cliStdout = readFileSync(result.record.stdoutPath, "utf8");
        // tsc -p prints its diagnostics then its extended diagnostics; keep
        // both ends of an oversized output.
        if (cliStdout.length > 1 << 20) cliStdout = cliStdout.slice(0, 1 << 19) + "\n…\n" + cliStdout.slice(-(1 << 19));
      } catch {
        cliStdout = null;
      }
    }
    const record = result.record ? { ...result.record, samples: undefined, sampleCount: result.record.samples?.length ?? 0 } : null;
    invocations.push({
      index,
      scenario: cell.scenario.id,
      setting: cell.setting.id,
      arm: step.arm,
      rep: step.rep,
      warmup: step.warmup,
      command,
      supervisorOut: supOut,
      supervisorExit: result.supervisorExit,
      supervisorSignal: result.supervisorSignal ?? null,
      supervisorReadError: result.readError ?? result.spawnError ?? null,
      supervisor: record,
      probeOut,
      phase,
      probe,
      probeReadError,
      cliStdout,
    });
    if (step.warmup && record?.killedBy === "memory") memoryKilled.set(cellArm, index);
    const end = record?.killedBy ? `killed:${record.killedBy}` : `exit ${record?.exitCode ?? "?"}`;
    log(
      `[${index + 1}/${plan.length}] ${step.key} ${step.arm} ${step.warmup ? "warmup" : "rep"} ${step.rep}: ${end} ` +
        `${record?.wallMs?.toFixed?.(0) ?? "?"} ms (${((Date.now() - t0) / 1000).toFixed(1)} s)`,
    );
  }

  const binariesAfter = {
    probe: sha256File(binaries.probe.pinned),
    counted: sha256File(binaries.counted.pinned),
    supervisor: sha256File(binaries.supervisor.pinned),
    tsc: sha256File(typescript.exe),
    tsApi: Object.fromEntries(Object.keys(typescript.apiSha256).map((f) => [f, sha256File(join(typescript.packageDir, f))])),
  };

  const run = {
    schema: RESULTS_SCHEMA,
    meta: {
      startedAt: new Date(started).toISOString(),
      finishedAt: new Date().toISOString(),
      options: { ...opts, out: outDir },
      argv,
      tree,
      buildInputs,
      buildInputsAfterBuild,
      harness,
      harnessAfter: harnessFingerprint(ROOT),
      host: hostInfo(),
      tuning,
      typescript,
      build,
      binaries,
      binariesAfter,
      expected: { file: EXPECTED_FILE, sha256: sha256File(EXPECTED_FILE), schema: expected.schema, measuredWith: expected.measuredWith },
      scenarios: Object.fromEntries(
        [...cells.entries()].map(([key, cell]) => [
          key,
          { id: cell.scenario.id, family: cell.scenario.family, note: cell.scenario.note, setting: cell.setting.id, inputs: cell.inputs, dir: cell.dir },
        ]),
      ),
      plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
    },
    invocations,
  };
  run.summary = summarize(run, expected, scenarios);
  const validation = validateRun(run, expected, scenarios);
  run.validation = validation;
  writeFileSync(join(outDir, "results.json"), JSON.stringify(run, null, 1));
  writeFileSync(join(outDir, "results.md"), renderMarkdown(run));
  log(`\nsemantic-perf: results ${join(outDir, "results.json")}`);
  log(`semantic-perf: report  ${join(outDir, "results.md")}`);
  log(`semantic-perf: validation ${validation.ok ? "PASSED" : "FAILED"} (${validation.failures.length} failure(s))`);
  for (const failure of validation.failures.slice(0, 40)) log(`  - ${failure}`);
  return validation.ok ? 0 : 1;
}

