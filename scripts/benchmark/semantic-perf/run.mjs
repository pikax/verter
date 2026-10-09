// The benchmark run: materialise every (scenario, setting), schedule every
// arm's warmup and measured invocations in counterbalanced order, run each
// under the supervisor, keep every record, then summarise, validate and
// report. See docs/contributing/semantic-benchmark.md.

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  ARMS,
  compactProbeRecord,
  DEFAULT_ARMS,
  invocationEnd,
  runLimits,
  supervisorDeadlineMs,
  warmRepeatsFor,
} from "./analyze.mjs";
import {
  BUILD_INPUTS,
  buildProblems,
  harnessFingerprint,
  buildVerterProbes,
  constructedEnv,
  envReceipt,
  RUNTIME_ENV_NAMES,
  hostInfo,
  pinBinary,
  probeIdentity,
  resolveTypeScript,
  sha256File,
  sha256Text,
  sourceTree,
} from "./provenance.mjs";
import { renderMarkdown } from "./report.mjs";
import {
  allScenarios,
  cliSource,
  scenariosForTier,
  SETTINGS,
  TIERS,
  tsconfigText,
} from "./scenarios.mjs";
import { sessionArmsOf, sessionInputs } from "./session-analyze.mjs";
import {
  INIT_ALIAS,
  sessionProblems,
  sessionScript,
  sessionsFor,
  sessionTsconfigText,
} from "./sessions.mjs";
import { summarize } from "./summary.mjs";
import { resolveSupervisor, runSupervised } from "./supervisor.mjs";
import { validateRun } from "./validate.mjs";
import { renderBiomeMarkdown, runBiome, summarizeBiome, validateBiome } from "./oss/biome.mjs";
import {
  renderOssMarkdown,
  runOss,
  selectCheckers,
  summarizeOss,
  validateOss,
} from "./oss/checkers.mjs";
import { loadTools } from "./oss/provision.mjs";
import { loadThenableExpected } from "./oss/thenable.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
export const ROOT = resolve(HERE, "..", "..", "..");
export const LIB_FILE = join(HERE, "lib", "bench-globals.d.ts");
export const EXPECTED_FILE = join(HERE, "expected.json");
export const RESULTS_SCHEMA = 1;

/**
 * The tiers' defaults (any option given explicitly wins). Every tier keeps
 * the fairness properties: fresh processes for every cold measurement, warm
 * repeats in one live process per (cell, arm), counterbalanced order, one
 * supervisor and budget for every arm. The whole-program `tsc -p` arms run
 * in standard and stress only.
 */
export const TIER_DEFAULTS = {
  quick: {
    arms: ["verter", "tsc-api", "verter-obs", "verter-observe", "verter-counted"],
    repeat: 3,
    warmup: 1,
    warmRepeats: 3,
    timeoutMs: 60_000,
    startupAllowanceMs: 10_000,
    estimate:
      "not yet measured with the session workloads and the observe build (62 s and 336 invocations on a Ryzen 9 7950X before them), plus a second release build",
  },
  standard: {
    arms: DEFAULT_ARMS,
    repeat: 3,
    warmup: 1,
    warmRepeats: 3,
    timeoutMs: 120_000,
    startupAllowanceMs: 10_000,
    estimate: "about 10-15 minutes (564 s on a Ryzen 9 7950X, 1152 invocations)",
  },
  stress: {
    arms: DEFAULT_ARMS,
    repeat: 3,
    warmup: 1,
    warmRepeats: 3,
    timeoutMs: 600_000,
    startupAllowanceMs: 30_000,
    estimate: "hours (tsc runs to its 8 GiB cap; multi-second Verter requests)",
  },
};

export const USAGE = `usage: node scripts/benchmark/semantic-perf.mjs [options]

  --tier <t>              quick (default) | standard | stress:
                            quick     one normal size per scenario series; Verter, tsc API, and the
                                      labelled observability and counting arms; ${TIER_DEFAULTS.quick.estimate}
                            standard  + the other normal sizes and tsc's limit sizes, + tsc -p in both
                                      thread modes; the baseline; ${TIER_DEFAULTS.standard.estimate}
                            stress    + Verter's limit and pathological sizes; ${TIER_DEFAULTS.stress.estimate}
                          every tier: 1 warmup + 3 measured fresh processes per arm, 3 warm repeats in one
                          live process per arm; deadline 60 s (quick), 120 s (standard), 600 s (stress)
  --out <dir>             output directory (default: target/semantic-perf/<timestamp>)
  --only <a,b>            only these scenarios and sessions (id or id prefix, from the whole catalog;
                          overrides --tier)
  --settings <s>          strict (default) | all  (the four strictNullChecks x noImplicitAny settings)
  --arms <a,b>            arms to run (default: ${DEFAULT_ARMS.join(",")})
  --repeat <n>            measured invocations per arm (tier default 3; odd counts are balanced to within one)
  --warmup <n>            warmup invocations per arm, run and validated but not measured (tier default 1)
  --warm-repeats <n>      in-process repeats in each arm's one live process (tier default 3)
  --mem-mb <n>            the engine memory budget, equal for both tools (default 8192): an engine whose
                          own peak exceeds it counts as exhausting it
  --infra-mb <n>          containment allowance above the budget for the process tree's other members
                          (tsc's node client, the statistics reader) (default 1024)
  --timeout-ms <n>        per-invocation deadline (tier default 60000 / 120000 / 600000); a hang fails
                          fast and is reported; only a whole-program tsc -p run that ran the whole deadline
                          counts as exhausting it (a probe's deadline kill is reported, unverified)
  --startup-allowance-ms <n>  added to the supervisor's deadline for process start (tier default 10000,
                          stress 30000)
  --allow-tuning          pass the caller's whole environment to the build and the probes (by default
                          both get a constructed one) and run despite Cargo configuration outside the
                          repository; the report is labelled tuned
  --supervisor <path>     verter-supervise executable (default: build crates/verter_supervise)
  --allow-sampled         consent to a sampled (not kernel-enforced) memory cap; required on macOS
  --typescript-from <dir> directory whose node_modules resolves typescript@7.0.2 (default: repository root)
  --lib-mode <m>          how Verter reads the library: root-file (default, as tsc does) | ambient
  --no-skip-after-kill    re-run every invocation of an arm whose warmup was killed at the memory cap
                          (by default the rest are recorded as skipped: a memory kill is deterministic)
  --oss [a,b]             also run the pinned open-source tools (all, or the named ones: tsz,
                          bamtiscript, ezno, biome), each in a separate section
                          (docs/contributing/semantic-benchmark-oss.md); provisioned into target/oss-tools
                          on first use. The type checkers run whole program on the measuring program beside
                          tsc -p and a one-shot Verter process answering the demand; Biome, which has
                          no type query, answers whether __Probe is Promise-like (its
                          noFloatingPromises decision) on the thenable catalog beside Verter and tsc
  --biome                 the Biome section alone (as --oss biome)
  --no-demand             skip the demand section (Verter vs the tsc API on each scenario's demanded
                          type); the other selected sections still run, tsc included, and the Verter
                          probes are still built for them
  --only-oss [a,b]        --oss --no-demand
  --only-biome            --biome --no-demand
  --no-tsc                run no tsc process in any section (the demand section's tsc API and tsc -p
                          arms, the OSS section's tsc -p references, the Biome section's tsc arm):
                          every answer is still classified against the measured reference, but
                          nothing is timed against tsc (to skip only the demand section: --no-demand)
  --help

Every tier also runs the session workloads (scripts/benchmark/semantic-perf/sessions.mjs): INCREMENTAL
edits, an EDITOR SESSION over an InputMenu-equivalent Vue component and CONCURRENT demands, each one live
process per invocation in the verter, verter-observe and tsc-api arms (tsc incremental: API snapshot reuse).
The verter-observe arm is the probe built with semantic-observe (capture compiled in) in its own target
directory; the production probe has it physically compiled out. --no-demand skips them with the demand
section, and --no-tsc drops their tsc-api arm.`;

export function parseArgs(argv) {
  const opts = {
    tier: "quick",
    out: null,
    only: [],
    settings: "strict",
    arms: DEFAULT_ARMS,
    repeat: 3,
    warmup: 1,
    warmRepeats: 3,
    memMb: 8192,
    infraMb: 1024,
    timeoutMs: 60_000,
    startupAllowanceMs: 10_000,
    allowTuning: false,
    supervisor: null,
    allowSampled: false,
    typescriptFrom: ROOT,
    libMode: "root-file",
    skipAfterKill: true,
  };
  const positive = (name, value, min = 1) => {
    const n = Number(value);
    if (!Number.isInteger(n) || n < min)
      throw new Error(`${name} must be an integer >= ${min}, got ${value}`);
    return n;
  };
  const given = new Set();
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    given.add(a);
    const next = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    switch (a) {
      case "--tier": {
        const v = next();
        if (!TIERS.includes(v)) throw new Error(`--tier must be one of ${TIERS.join(", ")}`);
        opts.tier = v;
        break;
      }
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
        for (const arm of arms)
          if (!ARMS[arm]) throw new Error(`unknown arm ${arm}; arms: ${DEFAULT_ARMS.join(", ")}`);
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
      case "--infra-mb":
        opts.infraMb = positive(a, next());
        break;
      case "--timeout-ms":
        opts.timeoutMs = positive(a, next());
        break;
      case "--startup-allowance-ms":
        opts.startupAllowanceMs = positive(a, next());
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
        if (!["root-file", "ambient"].includes(v))
          throw new Error(`--lib-mode must be root-file or ambient`);
        opts.libMode = v;
        break;
      }
      case "--no-skip-after-kill":
        opts.skipAfterKill = false;
        break;
      case "--oss":
      case "--only-oss": {
        // An optional list: the next argument, unless it is another option.
        const list = i + 1 < argv.length && !argv[i + 1].startsWith("--") ? argv[++i] : "";
        opts.oss = [...(opts.oss ?? []), ...list.split(",").filter(Boolean)];
        if (a === "--only-oss") opts.noDemand = true;
        break;
      }
      case "--biome":
      case "--only-biome":
        opts.biome = true;
        if (a === "--only-biome") opts.noDemand = true;
        break;
      case "--no-tsc":
        opts.noTsc = true;
        break;
      case "--no-demand":
        opts.noDemand = true;
        break;
      case "--help":
        opts.help = true;
        break;
      default:
        throw new Error(`unknown option ${a}\n${USAGE}`);
    }
  }
  // The tier's defaults, for every option not given explicitly.
  const defaults = TIER_DEFAULTS[opts.tier];
  const flag = {
    arms: "--arms",
    repeat: "--repeat",
    warmup: "--warmup",
    warmRepeats: "--warm-repeats",
    timeoutMs: "--timeout-ms",
    startupAllowanceMs: "--startup-allowance-ms",
  };
  for (const [key, name] of Object.entries(flag)) if (!given.has(name)) opts[key] = defaults[key];
  // --no-demand: the demand section (Verter vs tsc) runs no arm.
  if (opts.noDemand) {
    if (given.has("--arms"))
      throw new Error("--no-demand runs no demand-section arm; --arms does not apply");
    opts.arms = [];
  }
  if (opts.noTsc) {
    const tscArms = opts.arms.filter((arm) => ARMS[arm].tool === "tsc");
    if (given.has("--arms") && tscArms.length)
      throw new Error(`--no-tsc excludes the tsc arms given with --arms: ${tscArms.join(", ")}`);
    opts.arms = opts.arms.filter((arm) => ARMS[arm].tool !== "tsc");
  }
  // `--oss` names the tools: every one when no list is given; Biome runs
  // its own section (it answers another form of the demand).
  if (opts.oss) {
    const list = opts.oss;
    if (!list.length || list.includes("biome")) opts.biome = true;
    const checkers = list.filter((id) => id !== "biome");
    if (list.length && !checkers.length) delete opts.oss;
    else opts.oss = checkers;
    if (opts.oss) selectCheckers(loadTools(), opts.oss);
  }
  if (opts.noDemand && !opts.oss && !opts.biome)
    throw new Error("--no-demand leaves nothing to run: add --oss and/or --biome");
  return opts;
}

function loadExpected() {
  if (!existsSync(EXPECTED_FILE))
    throw new Error(`the measured reference ${EXPECTED_FILE} is missing; run measure-expected.mjs`);
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
 * Problems with a plan's balance. Over the measured rounds of a cell, every
 * pair of arms must run in each order equally often — exactly, for an even
 * number of rounds; with an odd number, off by at most one, and since cells
 * alternate which arm leads the extra round, the whole run is off by at most
 * one per pair.
 */
export function scheduleBalanceProblems(plan, arms) {
  const problems = [];
  const byCellRound = new Map();
  const rounds = new Map();
  for (const step of plan) {
    if (step.warmup) continue;
    const k = `${step.key}|${step.rep}`;
    if (!byCellRound.has(k)) byCellRound.set(k, []);
    byCellRound.get(k).push(step.arm);
    if (!rounds.has(step.key)) rounds.set(step.key, new Set());
    rounds.get(step.key).add(step.rep);
  }
  const counts = new Map();
  const total = new Map();
  for (const [k, order] of byCellRound) {
    const key = k.slice(0, k.lastIndexOf("|"));
    for (let i = 0; i < arms.length; i++) {
      for (let j = i + 1; j < arms.length; j++) {
        const a = arms[i];
        const b = arms[j];
        const c = `${key}|${a}|${b}`;
        const d = order.indexOf(a) < order.indexOf(b) ? 1 : -1;
        counts.set(c, (counts.get(c) ?? 0) + d);
        total.set(`${a}|${b}`, (total.get(`${a}|${b}`) ?? 0) + d);
      }
    }
  }
  for (const [c, n] of counts) {
    const key = c.split("|")[0];
    const allowed = (rounds.get(key)?.size ?? 0) % 2;
    if (Math.abs(n) > allowed)
      problems.push(`unbalanced order ${c} (${n > 0 ? "first" : "second"} by ${Math.abs(n)})`);
  }
  for (const [pair, n] of total)
    if (Math.abs(n) > 1) problems.push(`unbalanced order over the run ${pair} (by ${Math.abs(n)})`);
  return problems;
}

/**
 * Runtime and build variables that tune either tool away from its shipped
 * defaults. RUSTC and CARGO_INCREMENTAL are not among them: the build sets
 * both itself (see buildVerterProbes); CARGO_BUILD_JOBS changes only build
 * parallelism.
 */
export const TUNING_VARIABLES = [
  "RUSTC_WRAPPER",
  "RUSTC_WORKSPACE_WRAPPER",
  "RUSTC_BOOTSTRAP",
  "CC",
  "CXX",
  "CFLAGS",
  "CXXFLAGS",
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
      .filter(([k]) => {
        // Environment names are case-insensitive on Windows.
        const name = k.toUpperCase();
        if (name === "CARGO_BUILD_JOBS") return false;
        return (
          TUNING_VARIABLES.includes(name) ||
          /^CARGO_(PROFILE|TARGET|BUILD)_/.test(name) ||
          /^VERTER_/.test(name) ||
          // allocator and loader controls (glibc, macOS libmalloc, dyld, ld.so)
          /^(MALLOC|GLIBC_TUNABLES|LD_|DYLD_|MIMALLOC_|JEMALLOC|_RJEM_)/.test(name)
        );
      })
      .sort(([a], [b]) => a.localeCompare(b)),
  );
}

/**
 * Cargo configuration files outside the repository (in its ancestors or in
 * CARGO_HOME) that the build would read: each can override flags, the
 * compiler or profiles, so each counts as tuning.
 */
export function outsideCargoConfig(root) {
  const found = {};
  const check = (dir) => {
    for (const name of ["config.toml", "config"]) {
      const file = join(dir, ".cargo", name);
      if (existsSync(file)) found[`cargo config ${file}`] = sha256File(file).slice(0, 16);
    }
  };
  let dir = dirname(root);
  for (;;) {
    check(dir);
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  const cargoHome = process.env.CARGO_HOME ?? join(homedir(), ".cargo");
  for (const name of ["config.toml", "config"]) {
    const file = join(cargoHome, name);
    if (existsSync(file)) found[`cargo config ${file}`] = sha256File(file).slice(0, 16);
  }
  return found;
}

/** Power and thermal state, recorded for the report (never enforced). */
export function powerReceipt() {
  const run = (cmd, args) => {
    const out = spawnSync(cmd, args, { encoding: "utf8", timeout: 10_000 });
    return out.status === 0 ? out.stdout.trim().split("\n").slice(0, 4).join(" / ") : null;
  };
  if (process.platform === "darwin")
    return { battery: run("pmset", ["-g", "batt"]), thermal: run("pmset", ["-g", "therm"]) };
  if (process.platform === "win32") return { scheme: run("powercfg", ["/getactivescheme"]) };
  return {};
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
        argv: [
          binaries.probe.pinned,
          "run",
          "--job",
          writeJob(`${runBase}.job.json`, { ...baseJob, observability: false }),
          "--out",
          probeOut,
        ],
        probeOut,
      };
    case "verter-obs":
      return {
        argv: [
          binaries.probe.pinned,
          "run",
          "--job",
          writeJob(`${runBase}.job.json`, { ...baseJob, observability: true }),
          "--out",
          probeOut,
        ],
        probeOut,
      };
    case "verter-observe":
      return {
        argv: [
          binaries.observe.pinned,
          "run",
          "--job",
          writeJob(`${runBase}.job.json`, { ...baseJob, observability: false }),
          "--out",
          probeOut,
        ],
        probeOut,
      };
    case "verter-counted":
      return {
        argv: [
          binaries.counted.pinned,
          "run",
          "--job",
          writeJob(`${runBase}.job.json`, { ...baseJob, observability: false }),
          "--out",
          probeOut,
        ],
        probeOut,
      };
    case "tsc-api": {
      const { libMode: _unused, ...shared } = baseJob;
      const job = {
        ...shared,
        tsPackageDir: typescript.packageDir,
        tscExe: typescript.exe,
        statsExe: binaries.probe.pinned,
      };
      return {
        argv: [
          process.execPath,
          join(HERE, "tsc-probe.mjs"),
          "--job",
          writeJob(`${runBase}.job.json`, job),
          "--out",
          probeOut,
        ],
        probeOut,
      };
    }
    case "tsc-cli":
      return {
        argv: [
          typescript.exe,
          "-p",
          join(scenarioDir, "cli", "tsconfig.json"),
          "--extendedDiagnostics",
        ],
        probeOut: null,
      };
    case "tsc-cli-1":
      return {
        argv: [
          typescript.exe,
          "-p",
          join(scenarioDir, "cli", "tsconfig.json"),
          "--extendedDiagnostics",
          "--singleThreaded",
        ],
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
  // Children get a constructed environment, so the caller's tuning
  // variables cannot reach them; they are recorded as ignored. Only a tuned
  // run (--allow-tuning) inherits them, and is labelled with them. Cargo
  // configuration outside the repository is read by cargo whatever the
  // environment, so it always needs --allow-tuning.
  const callerTuning = tuningEnvironment(process.env);
  const cargoConfig = outsideCargoConfig(ROOT);
  if (Object.keys(cargoConfig).length && !opts.allowTuning) {
    throw new Error(
      `Cargo configuration outside the repository would tune the build (${Object.keys(cargoConfig).join(", ")}): the benchmark measures both tools as shipped. Remove it, or pass --allow-tuning to run a labelled tuned benchmark.`,
    );
  }
  const tuning = { ...(opts.allowTuning ? callerTuning : {}), ...cargoConfig };
  const ignoredTuning = opts.allowTuning ? [] : Object.keys(callerTuning);
  const powerAtStart = powerReceipt();
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
  const build = buildVerterProbes(ROOT, { inherit: opts.allowTuning });
  // Every probe runs in a constructed environment (a tuned run inherits the caller's).
  const runtimeEnv = constructedEnv(RUNTIME_ENV_NAMES, {}, { inherit: opts.allowTuning });
  const problems = buildProblems(build);
  if (problems.length)
    throw new Error(`the Verter probe is not a production build:\n  ${problems.join("\n  ")}`);
  const binDir = join(outDir, "bin");
  const binaries = {
    probe: pinBinary(build.executables.semantic_perf_probe, binDir, "semantic_perf_probe"),
    counted: pinBinary(
      build.executables.semantic_perf_probe_counted,
      binDir,
      "semantic_perf_probe_counted",
    ),
  };
  binaries.probe.identity = probeIdentity(binaries.probe.pinned);
  binaries.counted.identity = probeIdentity(binaries.counted.pinned);
  // The observe arm's probe: the same source with optional capture compiled in.
  let observeBuild = null;
  if (opts.arms.includes("verter-observe")) {
    log("semantic-perf: building the observe probe (semantic-observe compiled in)");
    observeBuild = buildVerterProbes(ROOT, { inherit: opts.allowTuning, observe: true });
    const observeProblems = buildProblems(observeBuild);
    if (observeProblems.length)
      throw new Error(
        `the observe probe is not the observe build:\n  ${observeProblems.join("\n  ")}`,
      );
    binaries.observe = pinBinary(
      observeBuild.executables.semantic_perf_probe,
      binDir,
      "semantic_perf_probe_observe",
    );
    binaries.observe.identity = probeIdentity(binaries.observe.pinned);
  }
  if (binaries.probe.identity.captureAvailable !== false)
    throw new Error("the production probe has optional semantic capture compiled in");
  if (binaries.observe && binaries.observe.identity.captureAvailable !== true)
    throw new Error("the observe probe does not have optional semantic capture compiled in");
  for (const [name, bin] of [
    ["the Verter probe", binaries.probe],
    ["the counted probe", binaries.counted],
    ...(binaries.observe ? [["the observe probe", binaries.observe]] : []),
  ]) {
    if (
      bin.identity.nativeArch !== bin.identity.targetArch ||
      !sameArchitecture(bin.identity.targetArch, process.arch) ||
      !typescript.platformPackage.endsWith(`-${process.arch}`)
    ) {
      throw new Error(
        `${name} is built for ${bin.identity.targetArch} on ${bin.identity.nativeArch} hardware, node runs ${process.arch} and tsc is ${typescript.platformPackage}: the tools must run natively on one architecture`,
      );
    }
  }
  const supervisorSource = resolveSupervisor(ROOT, opts.supervisor);
  binaries.supervisor = {
    ...pinBinary(supervisorSource.path, binDir, "verter-supervise"),
    origin: supervisorSource.origin,
  };
  // The probe's build inputs, read again after the build: a change between
  // the two reads means the binary's source is unknown.
  const buildInputsAfterBuild = sourceTree(ROOT, BUILD_INPUTS);

  const expected = loadExpected();
  const libText = readFileSync(LIB_FILE, "utf8");
  // --only picks from the whole catalog; otherwise the tier decides.
  const scenarios = opts.only.length
    ? allScenarios().filter((s) => opts.only.some((p) => s.id === p || s.id.startsWith(p)))
    : scenariosForTier(opts.tier);
  const sessions = sessionsFor(opts.tier, TIERS, opts.only);
  if (!scenarios.length && !sessions.length)
    throw new Error(`--only ${opts.only.join(",")} selects no scenario and no session`);
  for (const session of sessions) {
    const problems = sessionProblems(session);
    if (problems.length)
      throw new Error(`the session catalog is invalid:\n  ${problems.join("\n  ")}`);
  }
  const settings = opts.settings === "all" ? SETTINGS : SETTINGS.filter((s) => s.id === "strict");
  const cells = new Map();
  for (const scenario of scenarios) {
    for (const setting of settings) {
      const key = `${scenario.id}/${setting.id}`;
      cells.set(key, {
        scenario,
        setting,
        ...materialize(outDir, scenario, setting, opts, libText),
      });
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
        skipped: {
          after: memoryKilled.get(cellArm),
          reason: "a warmup of this scenario and arm was killed at the memory cap",
        },
      });
      log(
        `[${index + 1}/${plan.length}] ${step.key} ${step.arm} ${step.warmup ? "warmup" : "rep"} ${step.rep}: skipped (warmup killed at the memory cap)`,
      );
      continue;
    }
    const runDir = join(outDir, "runs", cell.scenario.id, cell.setting.id, step.arm);
    mkdirSync(runDir, { recursive: true });
    const runBase = join(runDir, `${step.warmup ? "warmup" : "rep"}-${step.rep}`);
    const { argv: command, probeOut } = commandFor(
      step.arm,
      {
        binaries,
        typescript,
        scenarioDir: cell.dir,
        baseJob: { ...cell.baseJob, warmRepeats: warmRepeatsFor(step, opts) },
      },
      runBase,
    );
    const supOut = `${runBase}.sup.json`;
    const t0 = Date.now();
    const spawnedAtMs = Date.now();
    const result = await runSupervised(binaries.supervisor.pinned, {
      // The engine budget plus the allowance for the tree's other members;
      // the engine's own peak is held to the budget when classifying.
      memMb: opts.memMb + opts.infraMb,
      // The engine's deadline plus an allowance for the process start
      // before the engine's first phase (see killAttributed).
      timeoutMs: supervisorDeadlineMs(opts),
      out: supOut,
      env: runtimeEnv,
      cwd: cell.dir,
      argv: command,
      allowSampled: opts.allowSampled,
    });
    let probe = null;
    let probeReadError = null;
    let phase = null;
    let phaseHistory = null;
    if (probeOut) {
      try {
        const marker = JSON.parse(readFileSync(`${probeOut}.phase`, "utf8"));
        phase = marker.phase ?? null;
        phaseHistory = marker.history ?? null;
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
      } catch {
        cliStdout = null;
      }
    }
    const record = result.record
      ? { ...result.record, samples: undefined, sampleCount: result.record.samples?.length ?? 0 }
      : null;
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
      spawnedAtMs,
      phase,
      phaseHistory,
      probe,
      probeReadError,
      cliStdout,
    });
    // Skip the rest of an arm only after its ENGINE exhausted the cap.
    const last = invocations.at(-1);
    if (
      step.warmup &&
      record?.killedBy === "memory" &&
      invocationEnd(last, runLimits(opts)).kind === "killed"
    )
      memoryKilled.set(cellArm, index);
    const end = record?.killedBy ? `killed:${record.killedBy}` : `exit ${record?.exitCode ?? "?"}`;
    log(
      `[${index + 1}/${plan.length}] ${step.key} ${step.arm} ${step.warmup ? "warmup" : "rep"} ${step.rep}: ${end} ` +
        `${record?.wallMs?.toFixed?.(0) ?? "?"} ms (${((Date.now() - t0) / 1000).toFixed(1)} s)`,
    );
  }

  const sessionRun = await runSessions(sessions, {
    opts,
    outDir,
    libText,
    binaries,
    typescript,
    runtimeEnv,
    log,
  });

  // The opt-in comparisons, each in its own counterbalanced schedule and
  // its own section, after the semantic run (which they never change).
  const tools = opts.oss || opts.biome ? loadTools() : null;
  const shared = {
    root: ROOT,
    outDir,
    opts,
    tools,
    supervisor: binaries.supervisor.pinned,
    runtimeEnv,
    schedule,
    runSupervised,
    log,
  };
  const ossResult = opts.oss
    ? await runOss({
        ...shared,
        cells,
        ids: selectCheckers(tools, opts.oss),
        verterProbe: binaries.probe.pinned,
        typescript,
        libText,
        tsconfigText,
        jobs: process.env.CARGO_BUILD_JOBS ?? null,
      })
    : null;
  const biomeResult = opts.biome
    ? await runBiome({
        ...shared,
        verterProbe: binaries.probe.pinned,
        typescript,
        libText,
        tsconfigText,
      })
    : null;

  const binariesAfter = {
    ...(binaries.observe ? { observe: sha256File(binaries.observe.pinned) } : {}),
    probe: sha256File(binaries.probe.pinned),
    counted: sha256File(binaries.counted.pinned),
    supervisor: sha256File(binaries.supervisor.pinned),
    tsc: sha256File(typescript.exe),
    tsApi: Object.fromEntries(
      Object.keys(typescript.apiSha256).map((f) => [f, sha256File(join(typescript.packageDir, f))]),
    ),
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
      host: { ...hostInfo(), power: { atStart: powerAtStart, atEnd: powerReceipt() } },
      tuning,
      environment: { runtime: envReceipt(runtimeEnv), inherited: opts.allowTuning, ignoredTuning },
      typescript,
      build,
      observeBuild,
      binaries,
      binariesAfter,
      expected: {
        file: EXPECTED_FILE,
        sha256: sha256File(EXPECTED_FILE),
        schema: expected.schema,
        measuredWith: expected.measuredWith,
      },
      scenarios: Object.fromEntries(
        [...cells.entries()].map(([key, cell]) => [
          key,
          {
            id: cell.scenario.id,
            family: cell.scenario.family,
            note: cell.scenario.note,
            setting: cell.setting.id,
            inputs: cell.inputs,
            dir: cell.dir,
          },
        ]),
      ),
      plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
      sessions: sessionRun.meta,
      sessionPlan: sessionRun.plan,
    },
    invocations,
    sessionInvocations: sessionRun.invocations,
  };
  run.summary = summarize(run, expected, scenarios, sessions);
  const validation = validateRun(run, expected, scenarios, { sessions });
  run.validation = validation;
  let ok = validation.ok;
  const extraFailures = [];
  if (ossResult) {
    ossResult.summary = summarizeOss(ossResult, expected, scenarios, run.meta.options);
    ossResult.validation = validateOss(ossResult, expected, scenarios, run.meta.options, schedule);
    run.oss = ossResult;
    ok &&= ossResult.validation.ok;
    extraFailures.push(...ossResult.validation.failures);
  }
  if (biomeResult) {
    const thenableExpected = loadThenableExpected();
    biomeResult.summary = summarizeBiome(biomeResult, thenableExpected, run.meta.options);
    biomeResult.validation = validateBiome(
      biomeResult,
      thenableExpected,
      run.meta.options,
      schedule,
    );
    run.biome = biomeResult;
    ok &&= biomeResult.validation.ok;
    extraFailures.push(...biomeResult.validation.failures);
  }
  writeFileSync(join(outDir, "results.json"), JSON.stringify(run, null, 1));
  let markdown = renderMarkdown(run);
  if (run.oss) markdown += "\n" + renderOssMarkdown(run.oss);
  if (run.biome) markdown += "\n" + renderBiomeMarkdown(run.biome);
  writeFileSync(join(outDir, "results.md"), markdown);
  log(`\nsemantic-perf: results ${join(outDir, "results.json")}`);
  log(`semantic-perf: report  ${join(outDir, "results.md")}`);
  log(
    `semantic-perf: validation ${validation.ok ? "PASSED" : "FAILED"} (${validation.failures.length} failure(s))`,
  );
  for (const failure of validation.failures.slice(0, 40)) log(`  - ${failure}`);
  if (ossResult || biomeResult) {
    log(
      `semantic-perf: opt-in sections ${extraFailures.length ? "FAILED" : "PASSED"} (${extraFailures.length} failure(s))`,
    );
    for (const failure of extraFailures.slice(0, 40)) log(`  - ${failure}`);
  }
  return ok ? 0 : 1;
}

/**
 * Run the session workloads: each (session, arm) invocation is one fresh
 * process running the whole script on its own copy of the project (an edit
 * writes the copy), under the same supervisor, budget and deadline as every
 * probe, in the counterbalanced schedule.
 */
export async function runSessions(
  sessions,
  { opts, outDir, libText, binaries, typescript, runtimeEnv, log },
) {
  const meta = {};
  const templates = new Map();
  for (const session of sessions) {
    const dir = join(outDir, "sessions", session.id, "template");
    mkdirSync(dir, { recursive: true });
    const files = {
      "lib.bench.d.ts": libText,
      "tsconfig.json": sessionTsconfigText(session),
      ...session.files,
    };
    for (const [file, text] of Object.entries(files)) writeFileSync(join(dir, file), text);
    templates.set(session.id, { dir, files: Object.keys(files) });
    meta[session.id] = {
      id: session.id,
      family: session.family,
      note: session.note,
      dir,
      inputs: sessionInputs(session, libText),
    };
  }
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const arms = sessionArmsOf(opts.arms);
  const plan = schedule(
    sessions.map((s) => s.id),
    arms,
    opts.repeat,
    opts.warmup,
  );
  const invocations = [];
  for (const [index, step] of plan.entries()) {
    const session = byId.get(step.key);
    const runDir = join(
      outDir,
      "sessions",
      session.id,
      step.arm,
      `${step.warmup ? "warmup" : "rep"}-${step.rep}`,
    );
    const projectDir = join(runDir, "project");
    mkdirSync(projectDir, { recursive: true });
    const template = templates.get(session.id);
    for (const file of template.files)
      copyFileSync(join(template.dir, file), join(projectDir, file));
    const job = {
      schema: 1,
      dir: projectDir,
      tsconfig: "tsconfig.json",
      lib: "lib.bench.d.ts",
      libMode: opts.libMode,
      files: Object.keys(session.files),
      initFile: session.initFile,
      initAlias: INIT_ALIAS,
      steps: sessionScript(session),
      observability: false,
    };
    const jobPath = join(runDir, "job.json");
    const sessionOut = join(runDir, "session.json");
    let command;
    if (step.arm === "tsc-api") {
      writeJob(jobPath, {
        ...job,
        tsPackageDir: typescript.packageDir,
        tscExe: typescript.exe,
        statsExe: binaries.probe.pinned,
      });
      command = [
        process.execPath,
        join(HERE, "tsc-session-probe.mjs"),
        "--job",
        jobPath,
        "--out",
        sessionOut,
      ];
    } else {
      writeJob(jobPath, job);
      const exe = step.arm === "verter-observe" ? binaries.observe.pinned : binaries.probe.pinned;
      command = [exe, "session", "--job", jobPath, "--out", sessionOut];
    }
    const supOut = join(runDir, "sup.json");
    const t0 = Date.now();
    const spawnedAtMs = Date.now();
    const result = await runSupervised(binaries.supervisor.pinned, {
      memMb: opts.memMb + opts.infraMb,
      timeoutMs: supervisorDeadlineMs(opts),
      out: supOut,
      env: runtimeEnv,
      cwd: projectDir,
      argv: command,
      allowSampled: opts.allowSampled,
    });
    let marker = null;
    try {
      marker = JSON.parse(readFileSync(`${sessionOut}.phase`, "utf8"));
    } catch {
      marker = null;
    }
    let record = null;
    let sessionReadError = null;
    try {
      record = JSON.parse(readFileSync(sessionOut, "utf8"));
    } catch (err) {
      sessionReadError = String(err.message ?? err);
    }
    const supervisor = result.record
      ? { ...result.record, samples: undefined, sampleCount: result.record.samples?.length ?? 0 }
      : null;
    invocations.push({
      index,
      sessionId: session.id,
      arm: step.arm,
      rep: step.rep,
      warmup: step.warmup,
      command,
      projectDir: projectDir.replace(/\\/g, "/"),
      supervisorOut: supOut,
      supervisorExit: result.supervisorExit,
      supervisorSignal: result.supervisorSignal ?? null,
      supervisorReadError: result.readError ?? result.spawnError ?? null,
      supervisor,
      sessionOut,
      spawnedAtMs,
      phase: marker?.phase ?? null,
      phaseHistory: marker?.history ?? null,
      session: record,
      sessionReadError,
    });
    const end = supervisor?.killedBy
      ? `killed:${supervisor.killedBy}`
      : `exit ${supervisor?.exitCode ?? "?"}`;
    log(
      `[session ${index + 1}/${plan.length}] ${session.id} ${step.arm} ${step.warmup ? "warmup" : "rep"} ${step.rep}: ${end} (${((Date.now() - t0) / 1000).toFixed(1)} s)`,
    );
  }
  return {
    meta,
    plan: plan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`),
    invocations,
  };
}
