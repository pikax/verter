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
// A workload gets a ratio only when both arms answer every witness it queries with
// the same outcome (the harness's untimed per-witness fingerprints). Lock evidence
// also requires the machine, toolchains and power state to be the runner class
// performance-gates.toml locks.
//
// Inside the same control bracket it also runs the candidate's cancellation probe
// (crates/verter_session/examples/signature_kernel_cancel_probe.rs) once per candidate
// invocation. The baseline has no caller-cancellable entry, so the probe's
// cancellation/restart distributions are recorded absolute, never gated — in
// aggregate and per injection point.
//
// The harness's three scalability sections (concurrent queries against a fixed host
// worker count, one caller against 1/2/4/8 host workers, and the full check at 1/2/4/8
// host workers) each get their own table.
//
//   node scripts/benchmark/signature-kernel-perf.mjs            # full run (lock evidence)
//   node scripts/benchmark/signature-kernel-perf.mjs --quick    # pipeline smoke test
//
//   node scripts/benchmark/signature-kernel-perf.mjs [options]
//
//   --out <dir>            output directory (default: <tmp>/sk-perf-<timestamp>)
//   --baseline <rev>       regression baseline (default: resolved by landing title, see below)
//   --candidate <rev>      candidate revision (default: HEAD)
//   --invocations <n>      invocations per arm, ABBA-interleaved (default: 4, the policy minimum)
//   --modules/--depth/--samples/--cold-samples/--soak <n>   forwarded to the harness
//   --host-workers <n>     the concurrent-query benchmark's fixed host worker count (forwarded)
//   --check-modules <n>    the scaling benchmarks' corpus size (forwarded)
//   --rounds <n>           cancellation rounds per injection point (forwarded to the probe only)
//   --exclude <Kind,Kind>  witness kinds NOT queried (forwarded); a workload whose witnesses the
//                          arms answer differently gets no ratio, so exclude the kinds the
//                          matched-work section names to compare the rest
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
import { pathToFileURL } from "node:url";

import { readGatesToml } from "../validate-performance-gates.mjs";

const HARNESS_REL = "crates/verter_session/examples/signature_kernel_bench.rs";
const CANCEL_PROBE_REL = "crates/verter_session/examples/signature_kernel_cancel_probe.rs";
// The harness arguments the cancellation probe shares.
const CANCEL_PROBE_ARGS = ["--modules", "--depth", "--cold-samples", "--rounds"];
// The arguments only the cancellation probe takes.
const CANCEL_PROBE_ONLY_ARGS = ["--rounds"];
const BASELINE_TITLE = "resolve effective tsconfig semantic options into the type environment";

// The locked runner class and statistics are READ from the gate file, never restated
// here, so this runner cannot drift from what the lock records.
const GATES = readGatesToml(
  fs.readFileSync(new URL("../../performance-gates.toml", import.meta.url), "utf8"),
).root;
const LOCKED_RUNNER = lockedTable("runner", [
  "class",
  "os",
  "cpu",
  "logical_cpus",
  "memory_bytes",
  "rust_toolchain",
  "node_runtime",
  "power_policy",
  "max_control_drift_percent",
]);
const LOCKED_STATISTICS = lockedTable("statistics", [
  "confidence",
  "bootstrap_resamples",
  "noise_multiplier",
  "interleave_policy",
]);

function lockedTable(name, keys) {
  const table = GATES[name];
  const missing = keys.filter((key) => table?.[key] === undefined);
  if (missing.length > 0)
    throw new Error(`performance-gates.toml [${name}] lacks ${missing.join(", ")}`);
  return table;
}

/** `interleave_policy`'s "at least N invocations per arm", as a number. */
function minInvocationsOf(policy) {
  const words = { two: 2, three: 3, four: 4, five: 5, six: 6, seven: 7, eight: 8 };
  const count = policy.match(/at least (\w+) invocations per arm/)?.[1];
  const value = words[count] ?? Number(count);
  if (!Number.isInteger(value))
    throw new Error("performance-gates.toml interleave_policy names no invocation minimum");
  return value;
}

const POLICY = {
  // The charter's 5% investigation gate (§12); the gate file locks no signature-kernel cell.
  investigationFloorPercent: 5,
  // Fewer invocations cannot measure between-invocation noise (one reads as zero).
  minInvocationsPerArm: minInvocationsOf(LOCKED_STATISTICS.interleave_policy),
  noiseMultiplier: LOCKED_STATISTICS.noise_multiplier,
  confidence: LOCKED_STATISTICS.confidence,
  bootstrapResamples: LOCKED_STATISTICS.bootstrap_resamples,
  maxControlDriftPercent: LOCKED_RUNNER.max_control_drift_percent,
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
      case "--host-workers":
      case "--check-modules":
      case "--rounds":
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
      "--rounds": "3",
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

/**
 * This machine, both trees' toolchains and the power state against the LOCKED runner
 * class of performance-gates.toml. Every mismatch refuses lock evidence: a session on
 * another class is a measurement, never lock evidence. Both sides are compared in one
 * canonical form, so the formatting of a command's output never decides a match.
 */
function checkLockedRunner(trees) {
  const observed = {
    os: { system: os.type(), release: os.release(), machine: os.machine() },
    cpu: canonicalCpu(os.cpus()[0]?.model ?? "unknown"),
    logical_cpus: os.cpus().length,
    memory_bytes: os.totalmem(),
    node: process.versions.node,
    // Each tree resolves its own pinned toolchain.
    rust: Object.fromEntries(
      Object.entries(trees).map(([arm, tree]) => [
        arm,
        rustcFacts(tryRun("rustc", ["-vV"], { cwd: tree }) ?? ""),
      ]),
    ),
    platform: process.platform,
  };
  if (process.platform === "darwin") {
    observed.power_source =
      (tryRun("pmset", ["-g", "batt"]) ?? "").match(/'([^']+)'/)?.[1] ?? "unknown";
    observed.low_power_mode =
      tryRun("pmset", ["-g"])?.match(/lowpowermode\s+(\d)/)?.[1] ?? "unknown";
  }
  return {
    class: LOCKED_RUNNER.class,
    expected: {
      os: LOCKED_RUNNER.os,
      cpu: LOCKED_RUNNER.cpu,
      logical_cpus: LOCKED_RUNNER.logical_cpus,
      memory_bytes: LOCKED_RUNNER.memory_bytes,
      rust_toolchain: LOCKED_RUNNER.rust_toolchain,
      node_runtime: LOCKED_RUNNER.node_runtime,
      power_policy: LOCKED_RUNNER.power_policy,
    },
    observed,
    mismatches: runnerMismatches(canonicalLockedRunner(LOCKED_RUNNER), observed),
  };
}

/** A CPU model with its incidental whitespace removed. */
function canonicalCpu(model) {
  return model.trim().replace(/\s+/g, " ");
}

/** `rustc -vV`'s release, commit hash and commit date. */
function rustcFacts(verbose) {
  const field = (name) => verbose.match(new RegExp(`^${name}: (.+)$`, "m"))?.[1]?.trim() ?? null;
  return { release: field("release"), hash: field("commit-hash"), date: field("commit-date") };
}

/** A `major.minor.patch` version, with or without a leading `v`, as numbers. */
function semver(text) {
  const parts = String(text).trim().replace(/^v/, "").split(".").map(Number);
  return parts.length === 3 && parts.every(Number.isInteger) ? parts : null;
}

/**
 * The locked `[runner]` table in canonical form. A lock value that does not parse is
 * a loud failure: the lock would otherwise match nothing, silently.
 */
function canonicalLockedRunner(runner) {
  const os = runner.os.trim().split(/\s+/);
  if (os.length !== 3)
    throw new Error(`performance-gates.toml runner.os is not "system release machine"`);
  const rust = runner.rust_toolchain.match(/^(\S+) \((\w+) (\d{4}-\d{2}-\d{2})\)$/);
  if (!rust)
    throw new Error(`performance-gates.toml runner.rust_toolchain is not "release (hash date)"`);
  const node = semver(runner.node_runtime);
  if (!node) throw new Error("performance-gates.toml runner.node_runtime is not a version");
  return {
    os: { system: os[0], release: os[1], machine: os[2] },
    cpu: canonicalCpu(runner.cpu),
    logical_cpus: runner.logical_cpus,
    memory_bytes: runner.memory_bytes,
    rust: { release: rust[1], hash: rust[2], date: rust[3] },
    node,
    power_policy: runner.power_policy,
  };
}

/** Every way the canonical `observed` facts fall outside the canonical locked class. */
function runnerMismatches(expected, observed) {
  const mismatches = [];
  for (const part of ["system", "release", "machine"]) {
    if (observed.os[part] !== expected.os[part])
      mismatches.push(`os ${part} is ${observed.os[part]}, locked ${expected.os[part]}`);
  }
  for (const key of ["cpu", "logical_cpus", "memory_bytes"]) {
    if (observed[key] !== expected[key])
      mismatches.push(`${key} is ${observed[key]}, locked ${expected[key]}`);
  }
  const node = semver(observed.node);
  if (!node || node.join(".") !== expected.node.join("."))
    mismatches.push(`node runtime is ${observed.node}, locked ${expected.node.join(".")}`);
  for (const [arm, rust] of Object.entries(observed.rust)) {
    // The lock records the commit's short hash; `rustc -vV` reports the full one.
    const same =
      rust.release === expected.rust.release &&
      rust.date === expected.rust.date &&
      typeof rust.hash === "string" &&
      rust.hash.startsWith(expected.rust.hash);
    if (!same)
      mismatches.push(
        `${arm} toolchain is ${rust.release} (${rust.hash?.slice(0, expected.rust.hash.length)} ${rust.date}), locked ${expected.rust.release} (${expected.rust.hash} ${expected.rust.date})`,
      );
  }
  // The power policy is prose; the machine-checkable clauses it states are enforced.
  const wantsAc = /AC power/i.test(expected.power_policy);
  const wantsLowPowerOff = /lowpowermode 0/i.test(expected.power_policy);
  if (wantsAc || wantsLowPowerOff) {
    if (observed.platform !== "darwin") {
      mismatches.push(`power state cannot be verified on ${observed.platform}`);
    } else {
      if (wantsAc && observed.power_source !== "AC Power")
        mismatches.push(`power source is ${observed.power_source}, locked AC Power`);
      if (wantsLowPowerOff && observed.low_power_mode !== "0")
        mismatches.push(`low-power mode is ${observed.low_power_mode}, locked 0`);
    }
  }
  return mismatches;
}

/** The edit workloads, each timed over the original state and its own edited state. */
const EDIT_WORKLOADS = ["local_edit", "declaration_edit", "augmentation_edit"];

/**
 * Matched work: every arm's per-witness OUTCOMES (completion, typed degradation or
 * refusal, and the answered type) compared witness by witness in every state. An
 * arm whose invocations disagree with each other is nondeterministic and matches
 * nothing.
 */
function compareOutcomes(runs) {
  const perArm = {};
  const nondeterministic = [];
  for (const arm of ["baseline", "candidate"]) {
    const documents = runs[arm].map((r) => r.document.outcomes);
    if (documents.some((d) => d === undefined))
      throw new Error(`the ${arm} harness recorded no outcomes`);
    const first = JSON.stringify(documents[0]);
    if (documents.some((d) => JSON.stringify(d) !== first)) nondeterministic.push(arm);
    perArm[arm] = documents[0];
  }
  const states = {};
  const names = new Set([...Object.keys(perArm.baseline), ...Object.keys(perArm.candidate)]);
  for (const state of [...names].sort()) {
    const base = perArm.baseline[state] ?? {};
    const cand = perArm.candidate[state] ?? {};
    const witnesses = [...new Set([...Object.keys(base), ...Object.keys(cand)])].sort();
    states[state] = {
      witnesses,
      mismatches: witnesses
        .filter((w) => base[w] !== cand[w])
        .map((w) => ({ witness: w, baseline: base[w] ?? null, candidate: cand[w] ?? null })),
    };
  }
  return { states, nondeterministic };
}

/**
 * Whether a workload times the same work in both arms: every witness it queries
 * has one outcome in both, in every state it reaches (the original corpus, and for
 * an edit workload its edited state, over the witnesses that edit re-queries).
 */
function workloadIsMatched(outcomes, name) {
  if (outcomes.nondeterministic.length > 0) return false;
  const original = outcomes.states.original;
  if (!original) return false;
  if (!EDIT_WORKLOADS.includes(name)) return original.mismatches.length === 0;
  const edited = outcomes.states[name];
  if (!edited || edited.mismatches.length > 0) return false;
  const queried = new Set(edited.witnesses);
  return !original.mismatches.some((m) => queried.has(m.witness));
}

/** p50 / p95 / p99 and the count of a sample set. */
function stats(xs) {
  return { p50: quantile(xs, 0.5), p95: quantile(xs, 0.95), p99: quantile(xs, 0.99), n: xs.length };
}

/**
 * One timed distribution compared across the arms: the pooled percentiles and, over
 * matched work, the median ratio, its hierarchical bootstrap interval, the gate
 * (max(5%, 2 × the baseline's between-invocation noise)) and the verdict. A ratio
 * across different answers is not a speedup or a regression: unmatched work reports
 * its distributions and no ratio, interval, gate or verdict.
 */
function timeComparison(baseline, candidate, matched, seed) {
  const base = baseline.flat();
  const cand = candidate.flat();
  const baseMedians = baseline.map(median);
  // Between-invocation noise of the baseline arm, relative to its median.
  const noisePercent =
    ((Math.max(...baseMedians) - Math.min(...baseMedians)) / median(baseMedians)) * 100;
  const threshold = matched
    ? Math.max(POLICY.investigationFloorPercent, POLICY.noiseMultiplier * noisePercent)
    : null;
  const ci = matched
    ? bootstrapRatio(baseline, candidate, POLICY.bootstrapResamples, POLICY.confidence, seed)
    : null;
  return {
    baseline_ns: stats(base),
    candidate_ns: stats(cand),
    matched,
    median_ratio: matched ? median(cand) / median(base) : null,
    ratio_ci: ci,
    baseline_noise_percent: noisePercent,
    gate_threshold_percent: threshold,
    verdict: matched ? verdictFor(ci, threshold) : "not comparable — the arms answer differently",
  };
}

/**
 * The harness's scalability sections: where each keeps its points, and the outcome
 * states its witnesses are fingerprinted in (the scaling benchmarks run the check
 * corpus, whose modules include the original ones).
 */
const SCALING_SECTIONS = {
  concurrent_queries: { points: "by_callers", states: ["original"] },
  scheduler_scaling: { points: "by_workers", states: ["check"] },
  full_check: { points: "by_workers", states: ["check"] },
};

/**
 * Every scalability section present in the harness output, per point and arm: the
 * wall-time distribution with its comparison, and the section's own figures —
 * queries per second and the per-query latency percentiles (the median across
 * invocations of each invocation's percentile), the speedup against the one-worker
 * point (from the pooled medians), files per second, and the CPU utilisation over
 * every sample (`Σ cpu / (Σ wall × workers)`, median across invocations; null where
 * the harness had no process CPU clock).
 */
function summarizeScaling(runs, outcomes) {
  const sections = {};
  let seed = 0x5ca1e;
  for (const [section, spec] of Object.entries(SCALING_SECTIONS)) {
    const first = runs.baseline[0].document[section];
    if (!first) continue;
    const matched =
      outcomes.nondeterministic.length === 0 &&
      spec.states.every((state) => outcomes.states[state]?.mismatches.length === 0);
    const pointsOf = (arm, key) => runs[arm].map((r) => r.document[section][spec.points][key]);
    const medianOrNull = (xs) =>
      xs.some((x) => x === null || x === undefined) ? null : median(xs);
    const points = {};
    for (const key of Object.keys(first[spec.points])) {
      const figures = (arm) => {
        const docs = pointsOf(arm, key);
        const out = {};
        if (docs[0].qps !== undefined) out.qps = median(docs.flatMap((d) => d.qps));
        if (docs[0].query_latency_ns !== undefined) {
          out.query_latency_ns = Object.fromEntries(
            ["p50", "p95", "p99"].map((q) => [q, median(docs.map((d) => d.query_latency_ns[q]))]),
          );
        }
        if (docs[0].files_per_second !== undefined)
          out.files_per_second = median(docs.flatMap((d) => d.files_per_second));
        if (docs[0].cpu_utilisation_total !== undefined)
          out.cpu_utilisation = medianOrNull(docs.map((d) => d.cpu_utilisation_total));
        return out;
      };
      points[key] = {
        ...timeComparison(
          pointsOf("baseline", key).map((d) => d.samples_ns),
          pointsOf("candidate", key).map((d) => d.samples_ns),
          matched,
          seed++,
        ),
        baseline: figures("baseline"),
        candidate: figures("candidate"),
      };
    }
    if (spec.points === "by_workers" && points["1"]) {
      for (const point of Object.values(points)) {
        point.baseline.speedup_vs_1 = points["1"].baseline_ns.p50 / point.baseline_ns.p50;
        point.candidate.speedup_vs_1 = points["1"].candidate_ns.p50 / point.candidate_ns.p50;
      }
    }
    const { by_callers: _callers, by_workers: _workers, ...facts } = first;
    sections[section] = { ...facts, matched, points };
  }
  return sections;
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
  let cancelProbe = null;
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
    // The probe is the candidate's own file: it needs the candidate's cancellable entry.
    const probePath = path.join(trees.candidate, CANCEL_PROBE_REL);
    if (fs.existsSync(probePath)) {
      cancelProbe = {
        sha256: sha256(fs.readFileSync(probePath)),
        binary: buildExample(
          trees.candidate,
          path.join(out, "target", "candidate"),
          "verter_session",
          "signature_kernel_cancel_probe",
        ),
      };
    } else {
      log(`the candidate has no ${CANCEL_PROBE_REL}; cancellation is not measured`);
    }
    if (!opts.skipControl) {
      controlBinary = buildExample(
        trees.baseline,
        path.join(out, "target", "baseline"),
        "verter_bench",
        "attribution_baseline",
      );
    }

    const harnessArgs = Object.entries(opts.forwarded)
      .filter(([flag]) => !CANCEL_PROBE_ONLY_ARGS.includes(flag))
      .flat();
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

    const cancelRuns = [];
    if (cancelProbe) {
      const probeArgs = Object.entries(opts.forwarded)
        .filter(([flag]) => CANCEL_PROBE_ARGS.includes(flag))
        .flat();
      for (let i = 0; i < opts.invocations; i++) {
        coolDown();
        for (const name of foreignProcesses()) foreignDuringSession.add(name);
        log(`cancellation probe ${i + 1}/${opts.invocations}: candidate`);
        const invocation = spawnSync(cancelProbe.binary, probeArgs, {
          cwd: trees.candidate,
          env: { ...process.env, SK_BENCH_REV: revs.candidate },
          encoding: "utf8",
          maxBuffer: 1 << 30,
          stdio: ["ignore", "pipe", "pipe"],
        });
        if (invocation.status !== 0) {
          const tail = (invocation.stderr || "").trim().split("\n").slice(-5).join("\n");
          throw new Error(
            `cancellation probe exited ${invocation.status ?? invocation.signal} on invocation ${i + 1}:\n${tail}`,
          );
        }
        const rawName = `cancel-${String(i + 1).padStart(2, "0")}-candidate.json`;
        const text = invocation.stdout.trim();
        fs.writeFileSync(path.join(out, "raw", rawName), text);
        rawDigests.push({ file: rawName, sha256: sha256(text) });
        cancelRuns.push(JSON.parse(text));
      }
    }

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

    const runnerCheck = checkLockedRunner(trees);
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
      cancelProbe,
      cancelRuns,
      runnerCheck,
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
  cancelProbe,
  cancelRuns,
  runnerCheck,
}) {
  const first = runs.baseline[0].document;
  const outcomes = compareOutcomes(runs);
  const workloadNames = Object.keys(first.workloads);
  const clustered = (arm, name) => runs[arm].map((r) => r.document.workloads[name].samples_ns);

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
    const allocOf = (arm) => median(runs[arm].map((r) => r.document.workloads[name].alloc_bytes));
    const allocCountOf = (arm) =>
      median(runs[arm].map((r) => r.document.workloads[name].alloc_count));
    workloads[name] = {
      ...timeComparison(
        clustered("baseline", name),
        clustered("candidate", name),
        workloadIsMatched(outcomes, name),
        0x5eed + index,
      ),
      alloc_bytes: { baseline: allocOf("baseline"), candidate: allocOf("candidate") },
      alloc_count: { baseline: allocCountOf("baseline"), candidate: allocCountOf("candidate") },
    };
  });

  const scaling = summarizeScaling(runs, outcomes);

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
  if (runnerCheck.mismatches.length > 0) {
    lockRefusals.push(
      `not the locked runner class ${runnerCheck.class}: ${runnerCheck.mismatches.join("; ")}`,
    );
  }
  if (outcomes.nondeterministic.length > 0) {
    lockRefusals.push(
      `outcomes differ between invocations of the ${outcomes.nondeterministic.join(" and ")} arm`,
    );
  }
  const unmatched = [
    ...workloadNames.filter((name) => !workloads[name].matched),
    ...Object.entries(scaling)
      .filter(([, section]) => !section.matched)
      .map(([name]) => name),
  ];
  if (unmatched.length > 0) lockRefusals.push(`unmatched work in ${unmatched.join(", ")}`);

  return {
    schema: 1,
    lock_evidence: lockRefusals.length === 0,
    lock_evidence_refusals: lockRefusals,
    revisions: { baseline: revs.baseline, candidate: revs.candidate },
    runner_check: runnerCheck,
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
    outcomes,
    workloads,
    scaling,
    soak,
    cancellation: summarizeCancellation(cancelProbe, cancelRuns),
    raw_logs: rawDigests,
  };
}

/** The candidate-only cancellation probe: absolute distributions, no ratio, no verdict. */
function summarizeCancellation(probe, runs) {
  if (!probe || runs.length === 0) return null;
  const distributions = {};
  for (const name of Object.keys(runs[0].workloads)) {
    const pooled = runs.flatMap((r) => r.workloads[name].samples_ns);
    const medians = runs
      .map((r) => r.workloads[name].samples_ns)
      .filter((xs) => xs.length > 0)
      .map(median);
    distributions[name] =
      pooled.length === 0
        ? { n: 0 }
        : {
            p50: quantile(pooled, 0.5),
            p95: quantile(pooled, 0.95),
            p99: quantile(pooled, 0.99),
            n: pooled.length,
            // Between-invocation spread of the medians, relative to their median.
            spread_percent: ((Math.max(...medians) - Math.min(...medians)) / median(medians)) * 100,
          };
  }
  // Per injection point, pooled across invocations; where each cancellation landed is
  // read against its own invocation's median cold request.
  const byFraction = (runs[0].by_fraction ?? []).map((point, index) => {
    const points = runs.map((r) => r.by_fraction[index]);
    const pooledOf = (name) => points.flatMap((p) => p[name].samples_ns);
    const landedFractions = runs.flatMap((r) =>
      r.by_fraction[index].landed_ns.samples_ns.map((ns) => ns / r.cold_request_median_ns),
    );
    const distribution = (xs) => (xs.length === 0 ? { n: 0 } : stats(xs));
    return {
      fraction: point.fraction,
      landed_fraction: distribution(landedFractions),
      completed_before_cancel: points.reduce((sum, p) => sum + p.completed_before_cancel, 0),
      cancel_stop: distribution(pooledOf("cancel_stop")),
      restart: distribution(pooledOf("restart")),
    };
  });
  return {
    probe: { path: CANCEL_PROBE_REL, sha256: probe.sha256 },
    invocations: runs.length,
    corpus: runs[0].corpus,
    fractions: runs[0].fractions,
    completed_before_cancel: runs.reduce((sum, r) => sum + r.completed_before_cancel, 0),
    distributions,
    by_fraction: byFraction,
  };
}

/** The three scalability tables, each section only when the harness recorded it. */
function renderScaling(scaling, ms) {
  const lines = [];
  const util = (x) => (x === null || x === undefined ? "n/a" : x.toFixed(2));
  const concurrent = scaling.concurrent_queries;
  if (concurrent) {
    const us = (ns) => String(Number((ns / 1e3).toPrecision(4)));
    const lat = (l) => `${us(l.p50)} / ${us(l.p95)} / ${us(l.p99)}`;
    lines.push(
      "",
      `## Concurrent query scalability (one warm host, ${concurrent.host_workers} host workers, N callers)`,
      "",
      `Each caller runs ${concurrent.sweeps_per_sample} full sweeps of ${concurrent.witnesses} witnesses per sample; the callers are created once, released by a barrier, and timed from the first start to the last finish.`,
      "",
      "| Callers | baseline q/s | candidate q/s | baseline query p50 / p95 / p99 (µs) | candidate query p50 / p95 / p99 (µs) | time ratio (95% CI) | verdict |",
      "|---|---|---|---|---|---|---|",
    );
    for (const [key, p] of Object.entries(concurrent.points)) {
      lines.push(
        `| ${key} | ${Math.round(p.baseline.qps)} | ${Math.round(p.candidate.qps)} | ${lat(p.baseline.query_latency_ns)} | ${lat(p.candidate.query_latency_ns)} | ${ratioCell(p)} | ${p.verdict} |`,
      );
    }
  }
  const scheduler = scaling.scheduler_scaling;
  if (scheduler) {
    lines.push(
      "",
      "## Internal scheduler scalability (one caller, N host workers)",
      "",
      `A fresh host per sample, loaded untimed; the sample is the cold first query of all ${scheduler.witnesses} witnesses of the ${scheduler.modules}-module check corpus. Speedup is against one worker.`,
      "",
      "| Workers | baseline p50 (ms) | candidate p50 (ms) | baseline speedup | candidate speedup | baseline CPU utilisation | candidate CPU utilisation | time ratio (95% CI) | verdict |",
      "|---|---|---|---|---|---|---|---|---|",
    );
    for (const [key, p] of Object.entries(scheduler.points)) {
      lines.push(
        `| ${key} | ${ms(p.baseline_ns.p50)} | ${ms(p.candidate_ns.p50)} | ${p.baseline.speedup_vs_1.toFixed(2)} | ${p.candidate.speedup_vs_1.toFixed(2)} | ${util(p.baseline.cpu_utilisation)} | ${util(p.candidate.cpu_utilisation)} | ${ratioCell(p)} | ${p.verdict} |`,
      );
    }
  }
  const check = scaling.full_check;
  if (check) {
    lines.push(
      "",
      "## Full-check throughput (N host workers, N callers)",
      "",
      `A fresh host per sample; the ${check.files} files of the check corpus dealt out to one caller per worker, each loading its files and answering their ${check.witnesses} witnesses in all.`,
      "",
      "| Workers | baseline files/s | candidate files/s | baseline p50 (ms) | candidate p50 (ms) | baseline CPU utilisation | candidate CPU utilisation | time ratio (95% CI) | verdict |",
      "|---|---|---|---|---|---|---|---|---|",
    );
    for (const [key, p] of Object.entries(check.points)) {
      lines.push(
        `| ${key} | ${p.baseline.files_per_second.toFixed(1)} | ${p.candidate.files_per_second.toFixed(1)} | ${ms(p.baseline_ns.p50)} | ${ms(p.candidate_ns.p50)} | ${util(p.baseline.cpu_utilisation)} | ${util(p.candidate.cpu_utilisation)} | ${ratioCell(p)} | ${p.verdict} |`,
      );
    }
  }
  if (scheduler || check) {
    lines.push(
      "",
      "CPU utilisation is process CPU time (every thread, user and system) over wall time × host workers, across every sample of the point; the callers' own CPU counts, so it can exceed 1.",
    );
  }
  return lines;
}

/** A workload's ratio and interval, or `n/a` when its work is not matched. */
function ratioCell(w) {
  return w.matched
    ? `${w.median_ratio.toFixed(3)} (${w.ratio_ci.lower.toFixed(3)}–${w.ratio_ci.upper.toFixed(3)})`
    : "n/a";
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
  const runner = s.runner_check;
  lines.push(
    `- Locked runner class \`${runner.class}\`: ${runner.mismatches.length === 0 ? "this machine, toolchain and power state match" : `**does not match** (${runner.mismatches.join("; ")})`}`,
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
  lines.push("");
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

  lines.push("## Matched work", "");
  lines.push(
    "Every witness's outcome — completion, the typed degradation or refusal, and the",
    "answered type rendered structurally — in the original corpus and in each edited",
    "state. A workload gets a ratio only when both arms have the same outcome on every",
    "witness it queries; otherwise it is reported as not comparable.",
    "",
  );
  const nondeterministic = s.outcomes.nondeterministic;
  if (nondeterministic.length > 0) {
    lines.push(
      `**Outcomes differ between invocations of the ${nondeterministic.join(" and ")} arm**; no workload is matched.`,
      "",
    );
  }
  lines.push("| State | witnesses | differing |", "|---|---|---|");
  for (const [state, o] of Object.entries(s.outcomes.states)) {
    lines.push(`| ${state} | ${o.witnesses.length} | ${o.mismatches.length} |`);
  }
  const differing = Object.entries(s.outcomes.states).flatMap(([state, o]) =>
    o.mismatches.map((m) => ({ state, ...m })),
  );
  if (differing.length > 0) {
    const clip = (text) =>
      text === null ? "(absent)" : text.length > 160 ? `${text.slice(0, 160)}…` : text;
    lines.push("", "First differing witnesses:", "");
    for (const m of differing.slice(0, 10)) {
      lines.push(
        `- \`${m.state}\` \`${m.witness}\`: baseline \`${clip(m.baseline)}\`; candidate \`${clip(m.candidate)}\``,
      );
    }
  }
  lines.push("");

  lines.push("## Latency (ms)", "");
  lines.push(
    "| Workload | baseline p50 / p95 / p99 | candidate p50 / p95 / p99 | ratio (95% CI) | gate | verdict |",
    "|---|---|---|---|---|---|",
  );
  for (const [name, w] of Object.entries(s.workloads)) {
    lines.push(
      `| ${name} | ${ms(w.baseline_ns.p50)} / ${ms(w.baseline_ns.p95)} / ${ms(w.baseline_ns.p99)} | ${ms(w.candidate_ns.p50)} / ${ms(w.candidate_ns.p95)} / ${ms(w.candidate_ns.p99)} | ${ratioCell(w)} | ${w.matched ? `±${w.gate_threshold_percent.toFixed(1)}%` : "n/a"} | ${w.verdict} |`,
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
    lines.push(
      `| ${name} | ${w.alloc_bytes.baseline} | ${w.alloc_bytes.candidate} | ${w.alloc_count.baseline} | ${w.alloc_count.candidate} |`,
    );
  }
  lines.push(...renderScaling(s.scaling ?? {}, ms));
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
  lines.push("", "## Cancellation and restart (candidate only, ms)", "");
  const c = s.cancellation;
  if (!c) {
    lines.push("Not measured: the candidate has no cancellation probe.");
  } else {
    const at = c.fractions.map((f) => `${Math.round(f * 100)}%`).join(", ");
    lines.push(
      `Probe \`${c.probe.path}\` sha256 \`${c.probe.sha256.slice(0, 16)}…\`; ${c.invocations} invocation(s), ${c.corpus.modules} modules, depth ${c.corpus.depth}. Cancellations land at ${at} of the median cold request; ${c.completed_before_cancel} completed before theirs landed.`,
      "",
      "| Distribution | p50 / p95 / p99 | n | between-invocation spread |",
      "|---|---|---|---|",
    );
    for (const [name, d] of Object.entries(c.distributions)) {
      lines.push(
        d.n === 0
          ? `| ${name} | n/a | 0 | n/a |`
          : `| ${name} | ${ms(d.p50)} / ${ms(d.p95)} / ${ms(d.p99)} | ${d.n} | ${d.spread_percent.toFixed(1)}% |`,
      );
    }
    if ((c.by_fraction ?? []).length > 0) {
      const cell = (d) => (d.n === 0 ? "n/a" : `${ms(d.p50)} / ${ms(d.p95)} / ${ms(d.p99)}`);
      lines.push(
        "",
        "Per injection point:",
        "",
        "| Injection point | landed (median, of cold) | cancel_stop p50 / p95 / p99 | stopped | completed first | restart p50 / p95 / p99 |",
        "|---|---|---|---|---|---|",
      );
      for (const p of c.by_fraction) {
        const landed =
          p.landed_fraction.n === 0 ? "n/a" : `${(p.landed_fraction.p50 * 100).toFixed(1)}%`;
        lines.push(
          `| ${Math.round(p.fraction * 100)}% | ${landed} | ${cell(p.cancel_stop)} | ${p.cancel_stop.n} | ${p.completed_before_cancel} | ${cell(p.restart)} |`,
        );
      }
    }
    lines.push(
      "",
      "`cold_request` is the uncancelled request on a fresh host; `cancel_stop` runs from",
      "`cancel()` to the request returning `Cancelled`; `restart` is the retry on the same",
      "host after a cancelled attempt. An injection point is its fraction of the invocation's",
      "median cold request, counted from the request's own start; `landed` is where its",
      "cancellations actually fell. The baseline has no caller-cancellable entry, so these",
      "are recorded, not gated.",
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

export {
  LOCKED_RUNNER,
  canonicalCpu,
  canonicalLockedRunner,
  compareOutcomes,
  minInvocationsOf,
  renderMarkdown,
  runnerMismatches,
  rustcFacts,
  summarize,
  workloadIsMatched,
};

// Run only as a script: importing the module (its summary and rendering helpers)
// must never start a session.
if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`[sk-perf] ${error.message}\n`);
    process.exit(1);
  }
}
