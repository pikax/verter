#!/usr/bin/env node
// Signature-kernel performance run — the §12 gates of docs/arch/signature-kernel.md,
// measured under the statistics policy of performance-gates.toml.
//
// Builds the SAME harness (crates/verter_session/examples/signature_kernel_bench.rs,
// pinned by blob digest) against two trees — the regression BASELINE (the tree
// immediately before the kernel train's options/admission block) and the CANDIDATE —
// with one toolchain and profile, waits for the registration's idle-machine protocol,
// runs them in alternating ABBA order, brackets the session with the locked control
// benchmark, and writes a PORTABLE summary: medians,
// percentiles, bootstrap confidence intervals, the regression gate, the soak plateau
// and digests of the raw logs. Raw machine-bound logs stay under --out, never in the
// tracked tree.
//
//   node scripts/benchmark/signature-kernel-perf.mjs [options]
//
//   --out <dir>            output directory (default: <tmp>/sk-perf-<timestamp>)
//   --baseline <rev>       regression baseline (default: resolved by landing title, see below)
//   --candidate <rev>      candidate revision (default: HEAD)
//   --invocations <n>      invocations per arm, ABBA-interleaved (default: 4, the policy minimum)
//   --modules/--depth/--samples/--cold-samples/--soak <n>   forwarded to the harness
//   --exclude <Kind,Kind>  witness kinds NOT queried (forwarded); exclude the kinds the census
//                          shows the arms answer differently, so every ratio is a matched workload
//   --settle <s>           open the session no sooner than this long after the builds (default 180)
//   --cooldown <s>         pause this long before each control and each invocation, so every
//                          measurement opens from the same thermal state (default 0; a fanless
//                          laptop throttles under a long session and drifts the control)
//   --skip-control         skip the control benchmark (a session without it is not lock evidence)
//   --quick                small corpus, 2 invocations, no control — a pipeline smoke test only
//   --keep                 keep the build worktrees; a later run with the same --out reuses them
//                          and skips the rebuild, so a session can open on a cool machine
//
// The default baseline is resolved from the candidate's history by the landing title of
// the options/admission block ("resolve effective tsconfig semantic options into the type
// environment") and taking its first parent. It is looked up at run time, never pinned
// here, so a rebase or squash does not strand it.

import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const HARNESS_REL = "crates/verter_session/examples/signature_kernel_bench.rs";
const BASELINE_TITLE = "resolve effective tsconfig semantic options into the type environment";
const POLICY = {
  // performance-gates.toml [statistics] + the charter's 5% investigation gate.
  investigationFloorPercent: 5,
  // performance-gates.toml interleave_policy: "at least four invocations per arm".
  // Fewer cannot measure between-invocation noise (one invocation reads as zero).
  minInvocationsPerArm: 4,
  noiseMultiplier: 2,
  confidence: 0.95,
  bootstrapResamples: 10000,
  maxControlDriftPercent: 3,
  // A soak whose late live heap exceeds its early live heap by more than this is
  // reported as growth rather than a plateau.
  soakPlateauTolerancePercent: 10,
  // The registration's idle-machine protocol: 1-minute load average below 2.00 and no
  // foreign cargo/rustc/nextest process. Checked before the session opens (waiting up
  // to idleWaitSeconds for the build to settle) and again before every invocation.
  idleMaxLoadAverage1m: 2.0,
  idleForeignProcesses: ["cargo", "rustc", "cargo-nextest"],
  idleWaitSeconds: 600,
  // One control execution before the session-start measurement, never read: the first
  // execution of a freshly built binary pays for page-in and on-access scanning.
  controlWarmupRuns: 1,
  // A machine's thermal state outlives its load average: two release builds leave a
  // fanless laptop hot for minutes after the load drops, and a control measured then
  // drifts against the session's end. The session opens no sooner than this after the
  // builds (a reused build still waits it out).
  settleSeconds: 180,
};

// ── arguments ────────────────────────────────────────────────────────────

function parseArgs(argv) {
  const opts = {
    invocations: POLICY.minInvocationsPerArm,
    forwarded: {},
    quick: false,
    skipControl: false,
    keep: false,
    settleSeconds: POLICY.settleSeconds,
    cooldownSeconds: 0,
  };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${flag} expects a value`);
      return v;
    };
    switch (flag) {
      case "--out":
        opts.out = value();
        break;
      case "--baseline":
        opts.baseline = value();
        break;
      case "--candidate":
        opts.candidate = value();
        break;
      case "--invocations":
        opts.invocations = Number(value());
        break;
      case "--modules":
      case "--depth":
      case "--samples":
      case "--cold-samples":
      case "--soak":
      case "--exclude":
        opts.forwarded[flag] = value();
        break;
      case "--settle":
        opts.settleSeconds = Number(value());
        break;
      case "--cooldown":
        opts.cooldownSeconds = Number(value());
        break;
      case "--skip-control":
        opts.skipControl = true;
        break;
      case "--quick":
        opts.quick = true;
        break;
      case "--keep":
        opts.keep = true;
        break;
      case "--help":
      case "-h":
        process.stdout.write(
          fs
            .readFileSync(new URL(import.meta.url))
            .toString()
            .split("\n")
            .filter((l) => l.startsWith("//"))
            .map((l) => l.slice(3))
            .join("\n") + "\n",
        );
        process.exit(0);
      default:
        throw new Error(`unknown option ${flag}`);
    }
  }
  if (opts.quick) {
    opts.invocations = Math.min(opts.invocations, 2);
    opts.skipControl = true;
    opts.forwarded = {
      "--modules": "4",
      "--depth": "4",
      "--samples": "6",
      "--cold-samples": "3",
      "--soak": "20",
      ...opts.forwarded,
    };
  }
  if (!Number.isInteger(opts.invocations) || opts.invocations < 1)
    throw new Error("--invocations must be a positive integer");
  if (!Number.isFinite(opts.settleSeconds) || opts.settleSeconds < 0)
    throw new Error("--settle must be a non-negative number of seconds");
  if (!Number.isFinite(opts.cooldownSeconds) || opts.cooldownSeconds < 0)
    throw new Error("--cooldown must be a non-negative number of seconds");
  if (opts.quick) opts.settleSeconds = 0;
  return opts;
}

// ── process helpers ──────────────────────────────────────────────────────

function run(cmd, args, options = {}) {
  return execFileSync(cmd, args, { encoding: "utf8", maxBuffer: 1 << 30, ...options }).trim();
}

function tryRun(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { encoding: "utf8", ...options });
  return result.status === 0 ? (result.stdout || "").trim() : null;
}

function sha256(data) {
  return createHash("sha256").update(data).digest("hex");
}

function log(line) {
  process.stderr.write(`[sk-perf] ${line}\n`);
}

const EXE = process.platform === "win32" ? ".exe" : "";

function sleepMs(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// ── idle-machine protocol ────────────────────────────────────────────────

/** Running processes the protocol forbids during a session (names without `.exe`). */
function foreignProcesses() {
  const listing =
    process.platform === "win32"
      ? (tryRun("tasklist", ["/FO", "CSV", "/NH"]) ?? "")
          .split("\n")
          .map((line) => line.split('","')[0].replace(/^"/, ""))
      : (tryRun("ps", ["-A", "-o", "comm="]) ?? "")
          .split("\n")
          .map((line) => path.basename(line.trim()));
  return listing
    .map((name) => name.replace(/\.exe$/i, ""))
    .filter((name) => POLICY.idleForeignProcesses.includes(name));
}

/** 1-minute load average, or null where the platform has none (Windows reports zeros). */
function loadAverage1m() {
  return process.platform === "win32" ? null : os.loadavg()[0];
}

function idleNow() {
  const load = loadAverage1m();
  const foreign = foreignProcesses();
  return {
    load_average_1m: load,
    foreign_processes: foreign,
    idle: foreign.length === 0 && (load === null || load < POLICY.idleMaxLoadAverage1m),
  };
}

/** Wait for the machine to settle after the builds; report what was observed. */
function waitForIdle(waitSeconds) {
  const started = Date.now();
  let observed = idleNow();
  while (!observed.idle && Date.now() - started < waitSeconds * 1000) {
    log(
      `waiting for an idle machine (load ${observed.load_average_1m?.toFixed(2) ?? "n/a"}, foreign ${observed.foreign_processes.join(", ") || "none"})…`,
    );
    sleepMs(15000);
    observed = idleNow();
  }
  return { ...observed, waited_seconds: Math.round((Date.now() - started) / 1000) };
}

// ── statistics ───────────────────────────────────────────────────────────

function sorted(values) {
  return [...values].sort((a, b) => a - b);
}

function quantile(values, q) {
  const s = sorted(values);
  if (s.length === 0) return NaN;
  const pos = (s.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return s[lo] + (s[hi] - s[lo]) * (pos - lo);
}

const median = (values) => quantile(values, 0.5);

// Deterministic PRNG so the bootstrap interval is reproducible from the raw logs.
function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Bootstrap CI of median(candidate) / median(baseline). */
function bootstrapRatio(baseline, candidate, resamples, confidence, seed) {
  // HIERARCHICAL: `baseline` and `candidate` are arrays of invocations, each an
  // array of samples. Samples inside one invocation share its process, heap and
  // thermal state, so they are not independent draws: resampling them pooled makes
  // the interval far more confident than an ABBA session supports. Each resample
  // draws the invocations with replacement (the outer level), then each drawn
  // invocation's own samples with replacement (the inner level).
  const rand = mulberry32(seed);
  const pick = (xs) => xs[Math.floor(rand() * xs.length)];
  const draw = (invocations) => {
    const pooled = [];
    for (let i = 0; i < invocations.length; i++) {
      const samples = pick(invocations);
      for (let j = 0; j < samples.length; j++) pooled.push(pick(samples));
    }
    return pooled;
  };
  const ratios = new Array(resamples);
  for (let r = 0; r < resamples; r++) ratios[r] = median(draw(candidate)) / median(draw(baseline));
  const tail = (1 - confidence) / 2;
  return { lower: quantile(ratios, tail), upper: quantile(ratios, 1 - tail) };
}

/**
 * The verdict for a time ratio (candidate / baseline; below 1 is faster) with its
 * interval, against a gate of ±`gate` percent. "within noise" is reserved for an
 * interval that includes parity; an interval that excludes it is a MEASURED
 * difference, placed against the gate.
 */
function verdictFor(ci, gate) {
  const band = gate / 100;
  const g = `±${gate.toFixed(1)}%`;
  if (ci.lower <= 1 && ci.upper >= 1) return "within noise";
  if (ci.lower > 1) {
    if (ci.lower > 1 + band) return `REGRESSION beyond the ${g} gate — investigate`;
    if (ci.upper > 1 + band)
      return `measured regression, possibly beyond the ${g} gate — investigate`;
    return `measured regression within the ${g} gate`;
  }
  if (ci.upper < 1 - band) return `improvement beyond the ${g} gate`;
  if (ci.lower < 1 - band) return `measured improvement, possibly beyond the ${g} gate`;
  return `measured improvement within the ${g} gate`;
}

// ── git / build ──────────────────────────────────────────────────────────

function resolveBaseline(repo, candidate) {
  const landing = tryRun(
    "git",
    ["log", "--format=%H", "--fixed-strings", `--grep=${BASELINE_TITLE}`, "-1", candidate],
    { cwd: repo },
  );
  if (!landing) {
    throw new Error(
      `could not find the baseline landing ("${BASELINE_TITLE}") in the history of ${candidate}; pass --baseline <rev>`,
    );
  }
  return run("git", ["rev-parse", `${landing}^`], { cwd: repo });
}

function addWorktree(repo, dir, rev) {
  run("git", ["worktree", "add", "--detach", "--force", dir, rev], {
    cwd: repo,
    stdio: ["ignore", "pipe", "pipe"],
  });
}

/** Whether `dir` is itself the top of a git worktree (not merely inside some repository). */
function isOwnWorktree(dir) {
  const top = fs.existsSync(dir)
    ? tryRun("git", ["-C", dir, "rev-parse", "--show-toplevel"])
    : null;
  // Real paths: git reports a resolved top (macOS temp dirs sit behind /var → /private/var).
  return top !== null && fs.realpathSync(top) === fs.realpathSync(dir);
}

/**
 * Make `dir` a worktree at `rev`. A tree kept by an earlier `--keep` run is reused
 * as is when it is already at `rev` (its build is then a no-op), and moved to `rev`
 * in place otherwise (the build recompiles only what changed).
 */
function prepareTree(repo, dir, rev, arm) {
  if (!isOwnWorktree(dir)) {
    addWorktree(repo, dir, rev);
    return;
  }
  if (tryRun("git", ["-C", dir, "rev-parse", "HEAD"]) === rev) {
    log(`reusing the kept ${arm} tree`);
    return;
  }
  log(`moving the kept ${arm} tree to ${rev.slice(0, 12)}`);
  run("git", ["-C", dir, "checkout", "--detach", "--force", rev], {
    stdio: ["ignore", "pipe", "pipe"],
  });
}

function removeWorktree(repo, dir) {
  tryRun("git", ["worktree", "remove", "--force", dir], { cwd: repo });
}

function buildExample(tree, targetDir, pkg, example) {
  log(`building ${pkg}/${example} in ${path.basename(tree)} (release)…`);
  const result = spawnSync("cargo", ["build", "--release", "-p", pkg, "--example", example], {
    cwd: tree,
    stdio: ["ignore", "inherit", "inherit"],
    env: { ...process.env, CARGO_TARGET_DIR: targetDir },
  });
  if (result.status !== 0) throw new Error(`cargo build of ${pkg}/${example} failed in ${tree}`);
  return path.join(targetDir, "release", "examples", `${example}${EXE}`);
}

// ── machine / session facts ──────────────────────────────────────────────

/** The OS thermal report, one line, or null where there is none. */
function thermalReport() {
  if (process.platform !== "darwin") return null;
  const report = tryRun("pmset", ["-g", "therm"]);
  return report ? report.replace(/\s+/g, " ").trim() : null;
}

function machineFacts() {
  const facts = {
    platform: process.platform,
    arch: process.arch,
    os_release: os.release(),
    cpu_model: os.cpus()[0]?.model ?? "unknown",
    logical_cpus: os.cpus().length,
    memory_bytes: os.totalmem(),
    rustc: tryRun("rustc", ["-V"]),
    cargo: tryRun("cargo", ["-V"]),
    node: process.version,
  };
  if (process.platform === "darwin") {
    facts.low_power_mode = tryRun("pmset", ["-g"])?.match(/lowpowermode\s+(\d)/)?.[1] ?? "unknown";
    facts.power_source = tryRun("pmset", ["-g", "batt"])?.split("\n")[0] ?? "unknown";
    facts.hw_model = tryRun("sysctl", ["-n", "hw.model"]);
  }
  return facts;
}

// ── main ─────────────────────────────────────────────────────────────────

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const repo = run("git", ["rev-parse", "--show-toplevel"]);
  const candidate = run("git", ["rev-parse", opts.candidate ?? "HEAD"], { cwd: repo });
  const baseline = opts.baseline
    ? run("git", ["rev-parse", opts.baseline], { cwd: repo })
    : resolveBaseline(repo, candidate);
  const out = path.resolve(
    opts.out ?? path.join(os.tmpdir(), `sk-perf-${new Date().toISOString().replace(/[:.]/g, "-")}`),
  );
  fs.mkdirSync(path.join(out, "raw"), { recursive: true });

  // The harness is taken from the WORKING TREE so the same bytes run in both arms,
  // including a harness change not yet committed; its digest pins the corpus.
  const harnessSource = fs.readFileSync(path.join(repo, HARNESS_REL));
  const harnessDigest = sha256(harnessSource);
  log(
    `baseline  ${baseline.slice(0, 12)}\ncandidate ${candidate.slice(0, 12)}\nharness   sha256:${harnessDigest.slice(0, 16)}…\nout       ${out}`,
  );

  const trees = {
    baseline: path.join(out, "trees", "baseline"),
    candidate: path.join(out, "trees", "candidate"),
  };
  const revs = { baseline, candidate };
  const binaries = {};
  let controlBinary = null;
  try {
    for (const arm of ["baseline", "candidate"]) {
      prepareTree(repo, trees[arm], revs[arm], arm);
      const harnessPath = path.join(trees[arm], HARNESS_REL);
      fs.mkdirSync(path.dirname(harnessPath), { recursive: true });
      // Rewriting identical bytes would touch the mtime and relink the harness.
      const current = fs.existsSync(harnessPath) ? fs.readFileSync(harnessPath) : null;
      if (!current || !current.equals(harnessSource)) fs.writeFileSync(harnessPath, harnessSource);
      binaries[arm] = buildExample(
        trees[arm],
        path.join(out, "target", arm),
        "verter_session",
        "signature_kernel_bench",
      );
    }
    if (!opts.skipControl) {
      controlBinary = buildExample(
        trees.baseline,
        path.join(out, "target", "baseline"),
        "verter_bench",
        "attribution_baseline",
      );
    }

    const harnessArgs = Object.entries(opts.forwarded).flat();
    const control = () => {
      const text = run(controlBinary, ["--files", "40", "--runs", "30"], {
        cwd: trees.baseline,
        stdio: ["ignore", "pipe", "ignore"],
      });
      const value = Number(text.match(/wall_median_ms\s+([\d.]+)/)?.[1]);
      if (!Number.isFinite(value)) throw new Error("control benchmark printed no wall_median_ms");
      return value;
    };

    if (opts.settleSeconds > 0) {
      log(`settling ${opts.settleSeconds}s after the builds…`);
      sleepMs(opts.settleSeconds * 1000);
    }
    const idleAtStart = waitForIdle(opts.quick ? 0 : POLICY.idleWaitSeconds);
    if (!idleAtStart.idle)
      log("the machine never became idle; the session runs but is not lock evidence");
    if (controlBinary) {
      for (let i = 0; i < POLICY.controlWarmupRuns; i++) control();
    }
    // Every measurement — both controls and every invocation — opens after the same
    // cool-down, so each starts from the same thermal state and the control drift
    // still catches a lasting change of the machine.
    const coolDown = () => {
      if (opts.cooldownSeconds > 0) sleepMs(opts.cooldownSeconds * 1000);
    };
    const thermalAtStart = thermalReport();
    coolDown();
    const controlStart = controlBinary
      ? (log("control benchmark (session start)…"), control())
      : null;

    // ABBA: A B B A A B B A … — each arm runs `invocations` times, alternated.
    const order = [];
    for (let i = 0; order.length < opts.invocations * 2; i++) {
      order.push(...(i % 2 === 0 ? ["baseline", "candidate"] : ["candidate", "baseline"]));
    }
    const runs = { baseline: [], candidate: [] };
    const rawDigests = [];
    // The load average cannot gate an invocation (the previous one's threads keep it
    // up), but a foreign build process voids the session wherever it appears.
    const foreignDuringSession = new Set();
    order.forEach((arm, index) => {
      coolDown();
      for (const name of foreignProcesses()) foreignDuringSession.add(name);
      log(`invocation ${index + 1}/${order.length}: ${arm}`);
      const started = Date.now();
      const invocation = spawnSync(binaries[arm], harnessArgs, {
        cwd: trees[arm],
        env: { ...process.env, SK_BENCH_REV: revs[arm] },
        encoding: "utf8",
        maxBuffer: 1 << 30,
        stdio: ["ignore", "pipe", "pipe"],
      });
      const rawName = `${String(index + 1).padStart(2, "0")}-${arm}.json`;
      if (invocation.stderr) {
        fs.writeFileSync(
          path.join(out, "raw", rawName.replace(/\.json$/, ".stderr.log")),
          invocation.stderr,
        );
      }
      if (invocation.status !== 0) {
        const tail = (invocation.stderr || "").trim().split("\n").slice(-5).join("\n");
        throw new Error(
          `${arm} harness exited ${invocation.status ?? invocation.signal} on invocation ${index + 1}:\n${tail}`,
        );
      }
      const text = invocation.stdout.trim();
      fs.writeFileSync(path.join(out, "raw", rawName), text);
      rawDigests.push({ file: rawName, sha256: sha256(text) });
      runs[arm].push({ document: JSON.parse(text), wall_ms: Date.now() - started });
    });

    coolDown();
    const controlEnd = controlBinary ? (log("control benchmark (session end)…"), control()) : null;
    const thermalAtEnd = thermalReport();
    for (const name of foreignProcesses()) foreignDuringSession.add(name);
    const controlDrift = controlBinary
      ? (Math.abs(controlEnd - controlStart) / controlStart) * 100
      : null;
    const idle = {
      at_start: idleAtStart,
      foreign_processes_during_session: [...foreignDuringSession].sort(),
      satisfied: idleAtStart.idle && foreignDuringSession.size === 0,
      // The OS's own thermal report, where it has one (macOS `pmset -g therm`).
      thermal: { at_start: thermalAtStart, at_end: thermalAtEnd },
    };
    const sessionVoid =
      (controlDrift !== null && controlDrift > POLICY.maxControlDriftPercent) || !idle.satisfied;

    const summary = summarize({
      opts,
      revs,
      harnessDigest,
      runs,
      rawDigests,
      controlStart,
      controlEnd,
      controlDrift,
      idle,
      sessionVoid,
    });
    fs.writeFileSync(path.join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
    fs.writeFileSync(path.join(out, "summary.md"), renderMarkdown(summary));
    process.stdout.write(renderMarkdown(summary));
    log(`summary written to ${path.join(out, "summary.md")}`);
  } finally {
    if (!opts.keep) {
      for (const dir of Object.values(trees)) removeWorktree(repo, dir);
    }
  }
}

function summarize({
  opts,
  revs,
  harnessDigest,
  runs,
  rawDigests,
  controlStart,
  controlEnd,
  controlDrift,
  idle,
  sessionVoid,
}) {
  const first = runs.baseline[0].document;
  const workloadNames = Object.keys(first.workloads);
  const pooled = (arm, name) => runs[arm].flatMap((r) => r.document.workloads[name].samples_ns);
  const clustered = (arm, name) => runs[arm].map((r) => r.document.workloads[name].samples_ns);
  const perInvocationMedians = (arm, name) =>
    runs[arm].map((r) => median(r.document.workloads[name].samples_ns));

  const censusOf = (arm) => runs[arm][0].document.census;
  const censusMatches =
    JSON.stringify(censusOf("baseline")) === JSON.stringify(censusOf("candidate"));
  const byWitnessOf = (arm) => runs[arm][0].document.census_by_witness ?? {};
  const censusByWitness = {};
  for (const kind of Object.keys(byWitnessOf("baseline"))) {
    censusByWitness[kind] = {
      baseline: byWitnessOf("baseline")[kind],
      candidate: byWitnessOf("candidate")[kind],
    };
  }

  const workloads = {};
  workloadNames.forEach((name, index) => {
    const base = pooled("baseline", name);
    const cand = pooled("candidate", name);
    const baseMedians = perInvocationMedians("baseline", name);
    // Between-invocation noise of the baseline arm, relative to its median.
    const noisePercent =
      ((Math.max(...baseMedians) - Math.min(...baseMedians)) / median(baseMedians)) * 100;
    const threshold = Math.max(
      POLICY.investigationFloorPercent,
      POLICY.noiseMultiplier * noisePercent,
    );
    const ratio = median(cand) / median(base);
    const ci = bootstrapRatio(
      clustered("baseline", name),
      clustered("candidate", name),
      POLICY.bootstrapResamples,
      POLICY.confidence,
      0x5eed + index,
    );
    const verdict = verdictFor(ci, threshold);
    const stats = (xs) => ({
      p50: quantile(xs, 0.5),
      p95: quantile(xs, 0.95),
      p99: quantile(xs, 0.99),
      n: xs.length,
    });
    const allocOf = (arm) => median(runs[arm].map((r) => r.document.workloads[name].alloc_bytes));
    const allocCountOf = (arm) =>
      median(runs[arm].map((r) => r.document.workloads[name].alloc_count));
    workloads[name] = {
      baseline_ns: stats(base),
      candidate_ns: stats(cand),
      median_ratio: ratio,
      ratio_ci: ci,
      baseline_noise_percent: noisePercent,
      gate_threshold_percent: threshold,
      verdict,
      alloc_bytes: { baseline: allocOf("baseline"), candidate: allocOf("candidate") },
      alloc_count: { baseline: allocCountOf("baseline"), candidate: allocCountOf("candidate") },
    };
  });

  const throughput = {};
  for (const key of Object.keys(first.throughput_qps)) {
    throughput[key] = {
      baseline_qps: median(runs.baseline.map((r) => r.document.throughput_qps[key])),
      candidate_qps: median(runs.candidate.map((r) => r.document.throughput_qps[key])),
    };
  }

  const soak = {};
  for (const arm of ["baseline", "candidate"]) {
    // Skip the first quarter (warm-up); compare the second quarter with the last.
    const series = runs[arm].map((r) => r.document.soak_live_bytes);
    const q = (s, from, to) =>
      median(s.slice(Math.floor(s.length * from), Math.floor(s.length * to)));
    const early = median(series.map((s) => q(s, 0.25, 0.5)));
    const late = median(series.map((s) => q(s, 0.75, 1)));
    const growthPercent = ((late - early) / early) * 100;
    soak[arm] = {
      early_live_bytes: early,
      late_live_bytes: late,
      growth_percent: growthPercent,
      verdict:
        growthPercent > POLICY.soakPlateauTolerancePercent ? "GROWTH — investigate" : "plateau",
    };
  }

  const lockRefusals = [];
  if (opts.quick) lockRefusals.push("quick run");
  if (opts.skipControl) lockRefusals.push("control benchmark skipped");
  if (sessionVoid) lockRefusals.push("void session");
  if (opts.invocations < POLICY.minInvocationsPerArm) {
    lockRefusals.push(
      `too few invocations (${opts.invocations} per arm, policy minimum ${POLICY.minInvocationsPerArm})`,
    );
  }

  return {
    schema: 1,
    lock_evidence: lockRefusals.length === 0,
    lock_evidence_refusals: lockRefusals,
    revisions: { baseline: revs.baseline, candidate: revs.candidate },
    harness: { path: HARNESS_REL, sha256: harnessDigest, args: opts.forwarded },
    machine: machineFacts(),
    policy: POLICY,
    session: {
      invocations_per_arm: opts.invocations,
      order: "ABBA",
      settle_seconds: opts.settleSeconds,
      cooldown_seconds: opts.cooldownSeconds,
      control_warmup_runs: controlStart === null ? 0 : POLICY.controlWarmupRuns,
      control_wall_median_ms:
        controlStart === null ? null : { start: controlStart, end: controlEnd },
      control_drift_percent: controlDrift,
      idle,
      void: sessionVoid,
    },
    corpus: first.corpus,
    census: {
      baseline: censusOf("baseline"),
      candidate: censusOf("candidate"),
      matches: censusMatches,
      by_witness: censusByWitness,
    },
    workloads,
    throughput,
    soak,
    raw_logs: rawDigests,
  };
}

function renderMarkdown(s) {
  // Four significant figures: a per-query warm read is hundredths of a millisecond.
  const ms = (ns) => String(Number((ns / 1e6).toPrecision(4)));
  const pct = (x) => `${x >= 0 ? "+" : ""}${x.toFixed(1)}%`;
  const lines = [];
  lines.push("# Signature-kernel performance run", "");
  lines.push(
    `- Baseline \`${s.revisions.baseline.slice(0, 12)}\` · candidate \`${s.revisions.candidate.slice(0, 12)}\``,
  );
  lines.push(`- Harness \`${s.harness.path}\` sha256 \`${s.harness.sha256.slice(0, 16)}…\``);
  lines.push(
    `- Machine: ${s.machine.cpu_model}, ${s.machine.logical_cpus} logical CPUs, ${(s.machine.memory_bytes / 2 ** 30).toFixed(0)} GiB, ${s.machine.platform} ${s.machine.os_release}`,
  );
  lines.push(`- Toolchain: ${s.machine.rustc ?? "rustc ?"}; ${s.machine.node}`);
  if (s.machine.low_power_mode !== undefined)
    lines.push(`- Power: ${s.machine.power_source}; low-power mode ${s.machine.low_power_mode}`);
  const drift = s.session.control_drift_percent;
  const idle = s.session.idle;
  const voidReasons = [];
  if (drift !== null && drift > s.policy.maxControlDriftPercent)
    voidReasons.push("control drift exceeds the gate");
  if (!idle.satisfied) voidReasons.push("machine not idle");
  lines.push(
    `- Session: ${s.session.invocations_per_arm} invocations per arm, ABBA; control drift ${drift === null ? "not measured" : pct(drift)}${s.session.void ? ` — **SESSION VOID** (${voidReasons.join("; ")})` : ""}`,
  );
  const control = s.session.control_wall_median_ms;
  lines.push(
    `- Control: ${control === null ? "not run" : `${control.start.toFixed(2)} ms at start, ${control.end.toFixed(2)} ms at end`}, after a ${s.session.settle_seconds}s settle and ${s.session.control_warmup_runs} warm-up run(s); ${s.session.cooldown_seconds}s cool-down before each control and invocation`,
  );
  if (idle.thermal?.at_start || idle.thermal?.at_end) {
    lines.push(
      `- Thermal: at start \`${idle.thermal.at_start ?? "n/a"}\`; at end \`${idle.thermal.at_end ?? "n/a"}\``,
    );
  }
  lines.push(
    `- Idle: load average at start ${idle.at_start.load_average_1m === null ? "n/a on this platform" : idle.at_start.load_average_1m.toFixed(2)} after ${idle.at_start.waited_seconds}s; foreign build processes at start ${idle.at_start.foreign_processes.join(", ") || "none"}, during the session ${idle.foreign_processes_during_session.join(", ") || "none"}`,
  );
  lines.push(
    `- Lock evidence: **${s.lock_evidence ? "yes" : "no"}**${s.lock_evidence ? "" : ` (${s.lock_evidence_refusals.join("; ")})`}`,
  );
  lines.push(
    `- Corpus: ${s.corpus.modules} modules, depth ${s.corpus.depth}, ${s.corpus.witnesses} witnesses`,
    "",
  );

  lines.push("## Completion census", "");
  lines.push("| Arm | complete | degraded | refused |", "|---|---|---|---|");
  for (const arm of ["baseline", "candidate"]) {
    const c = s.census[arm];
    lines.push(`| ${arm} | ${c.complete} | ${c.degraded} | ${c.refused} |`);
  }
  lines.push(
    "",
    s.census.matches
      ? "Both arms answer the same witnesses the same way, so every timing below is a matched workload."
      : "**The arms answer differently.** Timings below compare different amounts of completed work: per the regression policy a difference in what is answered is reported separately and a ratio across it is not a speedup or a regression by itself.",
    "",
  );
  const kinds = Object.entries(s.census.by_witness ?? {});
  if (kinds.length > 0) {
    const cell = (c) => `${c.complete} / ${c.degraded} / ${c.refused}`;
    lines.push("Per witness kind (complete / degraded / refused):", "");
    lines.push("| Witness | baseline | candidate | |", "|---|---|---|---|");
    for (const [kind, arms] of kinds) {
      const same = JSON.stringify(arms.baseline) === JSON.stringify(arms.candidate);
      lines.push(
        `| ${kind} | ${cell(arms.baseline)} | ${cell(arms.candidate)} | ${same ? "" : "**differs**"} |`,
      );
    }
    lines.push("");
  }

  lines.push("## Latency (ms)", "");
  lines.push(
    "| Workload | baseline p50 / p95 / p99 | candidate p50 / p95 / p99 | ratio (95% CI) | gate | verdict |",
    "|---|---|---|---|---|---|",
  );
  for (const [name, w] of Object.entries(s.workloads)) {
    if (name.startsWith("throughput_")) continue;
    lines.push(
      `| ${name} | ${ms(w.baseline_ns.p50)} / ${ms(w.baseline_ns.p95)} / ${ms(w.baseline_ns.p99)} | ${ms(w.candidate_ns.p50)} / ${ms(w.candidate_ns.p95)} / ${ms(w.candidate_ns.p99)} | ${w.median_ratio.toFixed(3)} (${w.ratio_ci.lower.toFixed(3)}–${w.ratio_ci.upper.toFixed(3)}) | ±${w.gate_threshold_percent.toFixed(1)}% | ${w.verdict} |`,
    );
  }
  lines.push(
    "",
    "Ratio is candidate / baseline time (below 1 is faster). The 95% interval is a",
    "hierarchical bootstrap: invocations are resampled first, then each drawn",
    "invocation's own samples. The gate is max(5%, 2 × the baseline's between-invocation",
    "noise). **within noise** — the interval includes parity; **measured",
    "regression / improvement** — the interval excludes parity, placed within or beyond",
    "the gate; an interval that crosses the gate is flagged for investigation.",
  );
  lines.push("", "## Allocation (per workload pass, median across invocations)", "");
  lines.push(
    "| Workload | baseline bytes | candidate bytes | baseline allocs | candidate allocs |",
    "|---|---|---|---|---|",
  );
  for (const [name, w] of Object.entries(s.workloads)) {
    if (name.startsWith("throughput_")) continue;
    lines.push(
      `| ${name} | ${w.alloc_bytes.baseline} | ${w.alloc_bytes.candidate} | ${w.alloc_count.baseline} | ${w.alloc_count.candidate} |`,
    );
  }
  lines.push(
    "",
    "## Throughput (queries/s, one warm host, N threads over N scheduler workers)",
    "",
  );
  lines.push(
    "| Workers | baseline | candidate | time ratio (95% CI) | verdict |",
    "|---|---|---|---|---|",
  );
  for (const [key, t] of Object.entries(s.throughput)) {
    // The throughput workload's own time samples carry the interval and verdict.
    const w = s.workloads[key];
    const timed = w
      ? `${w.median_ratio.toFixed(3)} (${w.ratio_ci.lower.toFixed(3)}–${w.ratio_ci.upper.toFixed(3)}) | ${w.verdict}`
      : "n/a | n/a";
    lines.push(
      `| ${key.replace("throughput_w", "")} | ${t.baseline_qps} | ${t.candidate_qps} | ${timed} |`,
    );
  }
  lines.push("", "## Edit/revert soak (live heap)", "");
  lines.push(
    "| Arm | second-quarter median | last-quarter median | growth | verdict |",
    "|---|---|---|---|---|",
  );
  for (const arm of ["baseline", "candidate"]) {
    const k = s.soak[arm];
    lines.push(
      `| ${arm} | ${k.early_live_bytes} | ${k.late_live_bytes} | ${pct(k.growth_percent)} | ${k.verdict} |`,
    );
  }
  lines.push(
    "",
    "## Raw logs",
    "",
    ...s.raw_logs.map((r) => `- \`${r.file}\` sha256 \`${r.sha256}\``),
    "",
  );
  return lines.join("\n");
}

try {
  main();
} catch (error) {
  process.stderr.write(`[sk-perf] ${error.message}\n`);
  process.exit(1);
}
