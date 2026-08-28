#!/usr/bin/env node
// Cold-invocation session runner for B6_COMPILER_ROUTE_OVERHEAD calibration
// and holdout. Refuses to run unless the idle-machine protocol holds.
//
//   node docs/arch/refactor/rev11/evidence/B6/cell-lock/run-session.mjs \
//     --bin ./target/release/examples/route_overhead_baseline \
//     --out docs/arch/refactor/rev11/evidence/B6/cell-lock/calibration \
//     --invocations 30
//
// Does not choose thresholds. Writes raw TSV + a summary JSON of observed
// statistics. Absolute budgets live in pre-measure-registration.md.

import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import process from "node:process";

const SHORT_MIN = 30;
const WALL_ABS_NS = 20_000_000;
const RSS_ABS = 134_217_728;
const MAX_CONTROL_DRIFT_PERCENT = 3.0;

function usage() {
  process.stderr.write(
    "usage: run-session.mjs --bin <path> --out <dir> --invocations 30 --control <path> [--skip-idle-check]\n",
  );
}

// Commit a repository-relative path. `resolve` is for execution only;
// writing the absolute path into session artifacts leaked a machine root.
function recordedPath(p) {
  return relative(process.cwd(), resolve(p)).split("\\").join("/");
}

function parseArgs(argv) {
  const out = { invocations: SHORT_MIN, skipIdle: false };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === "--bin") out.bin = argv[++i];
    else if (a === "--out") out.out = argv[++i];
    else if (a === "--invocations") out.invocations = Number.parseInt(argv[++i], 10);
    else if (a === "--control") out.control = argv[++i];
    else if (a === "--skip-idle-check") out.skipIdle = true;
    else {
      usage();
      process.exit(2);
    }
  }
  if (
    !out.bin ||
    !out.out ||
    !out.control ||
    !Number.isInteger(out.invocations) ||
    out.invocations < SHORT_MIN
  ) {
    usage();
    process.exit(2);
  }
  return out;
}

function loadavg1() {
  const text = execFileSync("/usr/bin/uptime", { encoding: "utf8" });
  const m = text.match(/load averages?: ([0-9.]+)/i);
  if (!m) throw new Error(`cannot parse uptime: ${text.trim()}`);
  return Number.parseFloat(m[1]);
}

function lowPowerMode() {
  const text = execFileSync("/usr/bin/pmset", ["-g"], { encoding: "utf8" });
  const m = text.match(/lowpowermode\s+(\d+)/);
  return m ? m[1] : "unknown";
}

function foreignHeavyProcs() {
  const text = execFileSync("/bin/ps", ["-axo", "pid,comm"], { encoding: "utf8" });
  const self = String(process.pid);
  const hits = [];
  for (const line of text.split("\n").slice(1)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const sp = trimmed.indexOf(" ");
    const pid = trimmed.slice(0, sp).trim();
    const comm = trimmed.slice(sp + 1).trim();
    if (pid === self) continue;
    if (/(^|\/)(cargo|cargo-nextest|rustc|gate\.mjs)$/.test(comm) || comm.includes("gate.mjs")) {
      hits.push(`${pid} ${comm}`);
    }
  }
  return hits;
}

// Registration section 5 voids the SESSION if any idle condition fails, so every
// condition is re-checked before every measured step -- not just at session
// start. A foreign compiler that starts mid-session voids the session exactly as
// one present at the start would.
function assertStillIdle(label) {
  const load = loadavg1();
  const heavy = foreignHeavyProcs();
  const reasons = [];
  if (!(load < 2.0)) reasons.push(`loadavg1=${load} (need < 2.00)`);
  if (heavy.length > 0) reasons.push(`foreign heavy procs: ${heavy.join("; ")}`);
  if (reasons.length > 0) {
    process.stderr.write(`IDLE BROKEN at ${label}:\n  ${reasons.join("\n  ")}\n`);
    process.exit(3);
  }
}

function idleCheck() {
  const load = loadavg1();
  const lpm = lowPowerMode();
  const heavy = foreignHeavyProcs();
  const reasons = [];
  if (!(load < 2.0)) reasons.push(`loadavg1=${load} (need < 2.00)`);
  if (lpm !== "0") reasons.push(`lowpowermode=${lpm} (need 0)`);
  if (heavy.length > 0) reasons.push(`foreign heavy procs: ${heavy.join("; ")}`);
  if (reasons.length > 0) {
    process.stderr.write(`IDLE PROTOCOL FAILED:\n  ${reasons.join("\n  ")}\n`);
    process.exit(3);
  }
  return { load, lowpowermode: lpm };
}

function parseTimeL(stderr) {
  // Darwin `/usr/bin/time -l`: "6340608  maximum resident set size"
  const darwin = stderr.match(/([0-9]+)\s+maximum resident set size/i);
  if (darwin) return Number.parseInt(darwin[1], 10);
  const posix = stderr.match(/maximum resident set size\s*[:=]?\s*([0-9]+)/i);
  if (posix) return Number.parseInt(posix[1], 10);
  throw new Error(`no RSS in time -l output:\n${stderr}`);
}

// Control benchmark (registration section 5, runner.control_benchmark): the
// baseline arm itself, re-run at session start and end. It detects machine
// drift ACROSS the session; only the two medians' agreement is used, never
// their absolute value, so it does not need to reproduce any A6 number.
function runControl(controlBin, label) {
  const r = spawnSync(controlBin, ["--files", "40", "--runs", "30"], { encoding: "utf8" });
  if (r.status !== 0) {
    process.stderr.write(`control (${label}) failed status=${r.status}\n${r.stderr}\n`);
    process.exit(1);
  }
  const m = r.stdout.match(/^wall_median_ms\t([0-9.]+)$/m);
  if (!m) throw new Error(`control (${label}): no wall_median_ms in output:\n${r.stdout}`);
  const ms = Number.parseFloat(m[1]);
  process.stderr.write(`control ${label} wall_median_ms=${ms}\n`);
  return ms;
}

function parseSummary(stdout) {
  const rows = {};
  for (const line of stdout.split("\n")) {
    if (!line.startsWith("SUMMARY\t")) continue;
    const body = line.slice("SUMMARY\t".length);
    const eq = body.indexOf("=");
    if (eq < 0) continue;
    rows[body.slice(0, eq)] = body.slice(eq + 1);
  }
  for (const key of [
    "median_wall_ns",
    "compile_calls",
    "artifact_count",
    "output_digest",
  ]) {
    if (!(key in rows)) throw new Error(`missing SUMMARY ${key}`);
  }
  return rows;
}

function populationCv(values) {
  const n = values.length;
  const mean = values.reduce((a, b) => a + b, 0) / n;
  const varPop = values.reduce((acc, v) => acc + (v - mean) ** 2, 0) / n;
  const sd = Math.sqrt(varPop);
  return mean === 0 ? 0 : (100 * sd) / mean;
}

function median(values) {
  const s = [...values].sort((a, b) => a - b);
  const n = s.length;
  if (n % 2 === 1) return s[(n - 1) / 2];
  return (s[n / 2 - 1] + s[n / 2]) / 2;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const bin = resolve(args.bin);
  const outDir = resolve(args.out);
  mkdirSync(outDir, { recursive: true });

  const idle = args.skipIdle
    ? { load: loadavg1(), lowpowermode: lowPowerMode(), skipped: true }
    : idleCheck();

  if (!args.skipIdle) assertStillIdle("control start");
  const controlStart = runControl(resolve(args.control), "start");

  const walls = [];
  const rss = [];
  const digests = new Set();
  const raw = [];
  raw.push("invocation\twall_ns\tpeak_rss_bytes\tcompile_calls\tartifact_count\toutput_digest");

  for (let i = 1; i <= args.invocations; i += 1) {
    if (!args.skipIdle) assertStillIdle(`invocation ${i}`);
    const timed = spawnSync("/usr/bin/time", ["-l", bin, "--runs", "1", "--warmup", "1"], {
      encoding: "utf8",
    });
    if (timed.status !== 0) {
      process.stderr.write(
        `invocation ${i} failed status=${timed.status}\n${timed.stdout}\n${timed.stderr}\n`,
      );
      process.exit(1);
    }
    const summary = parseSummary(timed.stdout);
    const wall = Number.parseInt(summary.median_wall_ns, 10);
    const rssBytes = parseTimeL(timed.stderr);
    walls.push(wall);
    rss.push(rssBytes);
    digests.add(summary.output_digest);
    raw.push(
      `${i}\t${wall}\t${rssBytes}\t${summary.compile_calls}\t${summary.artifact_count}\t${summary.output_digest}`,
    );
    process.stderr.write(`inv ${i}/${args.invocations} wall_ns=${wall} rss=${rssBytes}\n`);
  }

  if (!args.skipIdle) assertStillIdle("control end");
  const controlEnd = runControl(resolve(args.control), "end");
  const controlDriftPercent =
    controlStart === 0 ? 0 : (100 * Math.abs(controlEnd - controlStart)) / controlStart;
  if (controlDriftPercent > MAX_CONTROL_DRIFT_PERCENT) {
    process.stderr.write(
      `SESSION VOID: control drift ${controlDriftPercent.toFixed(4)}% > ` +
        `${MAX_CONTROL_DRIFT_PERCENT}% (start=${controlStart}ms end=${controlEnd}ms)\n`,
    );
    process.exit(4);
  }

  const wallCv = populationCv(walls);
  const rssCv = populationCv(rss);
  const wallRel = Math.max(3.0, 2 * wallCv);
  const rssRel = Math.max(3.0, 2 * rssCv);
  const summary = {
    idle,
    control_bin: recordedPath(args.control),
    control_start_wall_median_ms: controlStart,
    control_end_wall_median_ms: controlEnd,
    control_drift_percent: controlDriftPercent,
    max_control_drift_percent: MAX_CONTROL_DRIFT_PERCENT,
    invocations: args.invocations,
    bin: recordedPath(args.bin),
    median_wall_ns: median(walls),
    min_wall_ns: Math.min(...walls),
    max_wall_ns: Math.max(...walls),
    mean_wall_ns: walls.reduce((a, b) => a + b, 0) / walls.length,
    wall_cv_percent: wallCv,
    wall_no_regression_percent: wallRel,
    max_peak_rss_bytes: Math.max(...rss),
    median_peak_rss_bytes: median(rss),
    rss_cv_percent: rssCv,
    rss_no_regression_percent: rssRel,
    unique_output_digests: [...digests],
    pre_registered_wall_abs_ns: WALL_ABS_NS,
    pre_registered_rss_abs: RSS_ABS,
    wall_under_abs: median(walls) <= WALL_ABS_NS,
    rss_under_abs: Math.max(...rss) <= RSS_ABS,
  };

  writeFileSync(resolve(outDir, "samples.tsv"), `${raw.join("\n")}\n`);
  writeFileSync(resolve(outDir, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
}

main();
