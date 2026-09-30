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
import { SETTINGS, selectScenarios, tsconfigText } from "./scenarios.mjs";
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
  --mem-mb <n>            per-invocation process-tree memory cap (default 8192)
  --timeout-ms <n>        per-invocation deadline (default 300000)
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
    timeoutMs: 300_000,
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
      case "--timeout-ms":
        opts.timeoutMs = positive(a, next());
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

/** The counterbalanced schedule: warmup rounds, then measured rounds. */
export function schedule(scenarioKeys, arms, repeat, warmup) {
  const out = [];
  const rounds = [...Array(warmup).keys()].map((i) => ({ rep: i, warmup: true }));
  for (let i = 0; i < repeat; i++) rounds.push({ rep: i, warmup: false });
  rounds.forEach((round, roundIndex) => {
    const keys = roundIndex % 2 === 0 ? scenarioKeys : [...scenarioKeys].reverse();
    keys.forEach((key, keyIndex) => {
      const order = (roundIndex + keyIndex) % 2 === 0 ? arms : [...arms].reverse();
      for (const arm of order) out.push({ key, arm, rep: round.rep, warmup: round.warmup });
    });
  });
  return out;
}

function materialize(outDir, scenario, setting, opts, libText) {
  const dir = join(outDir, "scenarios", scenario.id, setting.id);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "lib.bench.d.ts"), libText);
  writeFileSync(join(dir, "scenario.ts"), scenario.source);
  writeFileSync(join(dir, "tsconfig.json"), tsconfigText(setting));
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
      const job = { ...shared, tsPackageDir: typescript.packageDir, statsExe: binaries.probe.pinned };
      return {
        argv: [process.execPath, join(HERE, "tsc-probe.mjs"), "--job", writeJob(`${runBase}.job.json`, job), "--out", probeOut],
        probeOut,
      };
    }
    case "tsc-cli":
      return { argv: [typescript.exe, "-p", join(scenarioDir, "tsconfig.json"), "--extendedDiagnostics"], probeOut: null };
    case "tsc-cli-1":
      return {
        argv: [typescript.exe, "-p", join(scenarioDir, "tsconfig.json"), "--extendedDiagnostics", "--singleThreaded"],
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
      memMb: opts.memMb,
      timeoutMs: opts.timeoutMs,
      out: supOut,
      cwd: cell.dir,
      argv: command,
      allowSampled: opts.allowSampled,
    });
    let probe = null;
    let probeReadError = null;
    if (probeOut) {
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

