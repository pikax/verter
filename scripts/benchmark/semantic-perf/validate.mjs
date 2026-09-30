#!/usr/bin/env node
// Validator of a semantic-benchmark run. A run FAILS validation when:
//   - a binary is not the one named (TypeScript not 7.0.2, a probe not a
//     production release build, any pinned binary changed during the run,
//     the source tree changed across the build);
//   - an invocation record is missing, duplicated, out of plan or order, or
//     zero records exist for a planned (scenario, setting, arm);
//   - a child failed (non-zero exit, no or malformed probe record, lost
//     telemetry, supervisor error, unconsented sampled containment);
//   - repetitions of one arm disagree on the answer or its class;
//   - the tsc arm's answer (text or error-type flag) differs from the
//     measured reference — the reference or the arm is wrong;
//   - the stored summary differs from the summary recomputed from the raw
//     records (a claimed "matched" or a claimed speed verdict that the raw
//     answers do not support).
// A Verter answer that is wrong, partial, refused or killed does NOT fail
// validation: it is a finding, classified and excluded from every speed or
// memory comparison. `--require-all-matched` additionally fails the run
// unless every Verter answer matched tsc (or is a verified beyond-tsc
// answer), for a run meant as a baseline.
//
//   node scripts/benchmark/semantic-perf/validate.mjs <results.json> [--require-all-matched]

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  ARMS,
  compactProbeRecord,
  invocationEnd,
  parseCli,
  probeAnswer,
  probeRecordProblems,
} from "./analyze.mjs";
import { sameArchitecture, schedule, scheduleBalanceProblems } from "./run.mjs";
import {
  buildProblems,
  RECORDED_PACKAGES,
  sha256File,
  sha256Text,
  TYPESCRIPT_VERSION,
} from "./provenance.mjs";
import { allScenarios, cliSource, SETTINGS, tsconfigText } from "./scenarios.mjs";
import { MEASURING_SUFFIX } from "./measure-expected.mjs";
import { summarize } from "./summary.mjs";
import { supervisorRecordProblems } from "./supervisor.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));

/** Plan string for an invocation. */
function planEntry(inv) {
  return `${inv.scenario}/${inv.setting}|${inv.arm}|${inv.warmup ? "w" : "r"}${inv.rep}`;
}

function stable(value) {
  return JSON.stringify(value, (_key, v) =>
    typeof v === "number" && !Number.isInteger(v) ? Number(v.toPrecision(12)) : v,
  );
}

/**
 * Validate a run object. `expected` is the measured reference the run was
 * summarised against; `scenarios` the catalog it ran.
 */
export function validateRun(run, expected, scenarios, { requireAllMatched = false } = {}) {
  const failures = [];
  const warnings = [];
  const fail = (m) => failures.push(m);
  const meta = run.meta ?? {};

  // Provenance.
  const ts = meta.typescript ?? {};
  if (
    ts.version !== TYPESCRIPT_VERSION ||
    ts.platformVersion !== TYPESCRIPT_VERSION ||
    ts.versionText !== `Version ${TYPESCRIPT_VERSION}`
  ) {
    fail(
      `wrong binary: TypeScript ${ts.version}/${ts.platformVersion} (${ts.versionText}), not ${TYPESCRIPT_VERSION}`,
    );
  }
  if (!meta.build) fail("wrong binary: no Verter build record");
  else for (const p of buildProblems(meta.build)) fail(`wrong binary: ${p}`);
  const bins = meta.binaries ?? {};
  const after = meta.binariesAfter ?? {};
  if (!bins.probe || bins.probe.sha256 !== after.probe)
    fail("wrong binary: the Verter probe changed during the run");
  if (!bins.counted || bins.counted.sha256 !== after.counted)
    fail("wrong binary: the counted Verter probe changed during the run");
  if (!bins.supervisor || bins.supervisor.sha256 !== after.supervisor)
    fail("wrong binary: the supervisor changed during the run");
  if (ts.exeSha256 !== after.tsc) fail("wrong binary: the native tsc changed during the run");
  if (
    bins.probe?.identity?.debugAssertions !== false ||
    bins.probe?.identity?.instrumented !== false
  ) {
    fail("wrong binary: the Verter probe reports debug assertions or instrumentation");
  }
  if (bins.counted?.identity?.instrumented !== true)
    fail("wrong binary: the counted probe does not report its instrumentation");
  const t0 = meta.buildInputs ?? {};
  const t1 = meta.buildInputsAfterBuild ?? {};
  if (
    !t0.head ||
    t0.head !== t1.head ||
    t0.diffSha256 !== t1.diffSha256 ||
    t0.untrackedSha256 !== t1.untrackedSha256
  ) {
    fail(
      "wrong binary: the probe's build inputs changed while it was built, so its source is unknown",
    );
  }
  if (!meta.harness || stable(meta.harness) !== stable(meta.harnessAfter)) {
    fail("the harness changed during the run, so its invocations did not all run one method");
  }
  for (const name of RECORDED_PACKAGES)
    if (!meta.build?.packages?.[name]) fail(`wrong binary: no build record for ${name}`);
  for (const [f, sha] of Object.entries(ts.apiSha256 ?? {})) {
    if (after.tsApi?.[f] !== sha)
      fail(`wrong binary: the tsc API client ${f} changed during the run`);
  }
  const host = meta.host ?? {};
  for (const [name, bin] of [
    ["the Verter probe", bins.probe],
    ["the counted probe", bins.counted],
  ]) {
    if (
      !bin?.identity ||
      bin.identity.nativeArch !== bin.identity.targetArch ||
      !sameArchitecture(bin.identity.targetArch, host.arch) ||
      !String(ts.platformPackage ?? "").endsWith(`-${host.arch}`)
    ) {
      fail(
        `wrong binary: ${name} (${bin?.identity?.targetArch}), node (${host.arch}) and tsc (${ts.platformPackage}) are not one native architecture`,
      );
    }
  }
  const b = meta.build ?? {};
  if (
    !b.rustcPath ||
    !b.rustcSha256 ||
    b.env?.RUSTC !== b.rustcPath ||
    b.env?.CARGO_INCREMENTAL !== "0"
  ) {
    fail(
      "wrong binary: the build does not record the controlled compiler (RUSTC bound to the fingerprinted toolchain rustc, CARGO_INCREMENTAL=0)",
    );
  }
  if (Object.keys(meta.tuning ?? {}).length && !meta.options?.allowTuning) {
    fail(
      `tuning variables were set without --allow-tuning: ${Object.keys(meta.tuning).join(", ")}`,
    );
  }

  // The reference and the inputs: every cell's files are the catalog's, and
  // the reference was measured on exactly those sources and that library.
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  for (const [key, cell] of Object.entries(meta.scenarios ?? {})) {
    const scenario = byId.get(cell.id);
    const setting = SETTINGS.find((s) => s.id === cell.setting);
    if (!scenario || !setting) {
      fail(`${key}: not a catalog scenario and setting`);
      continue;
    }
    if (cell.inputs?.["scenario.ts"] !== sha256Text(scenario.source))
      fail(`${key}: scenario.ts is not the catalog's source`);
    if (cell.inputs?.["tsconfig.json"] !== sha256Text(tsconfigText(setting)))
      fail(`${key}: tsconfig.json is not the catalog's`);
    if (cell.inputs?.["cli/scenario.ts"] !== sha256Text(cliSource(scenario)))
      fail(`${key}: cli/scenario.ts is not the catalog's source plus the probe's use`);
    const measured = expected.scenarios?.[cell.id];
    if (!measured) warnings.push(`${key}: no measured reference`);
    else {
      if (measured.sourceSha256 !== sha256Text(scenario.source))
        fail(`${key}: the measured reference is stale (measured on a different source)`);
      if (expected.method?.libSha256 !== cell.inputs?.["lib.bench.d.ts"])
        fail(`${key}: the measured reference used a different library`);
      if (expected.method?.measuringSuffixSha256 !== sha256Text(MEASURING_SUFFIX))
        fail(`${key}: the measured reference used a different measuring method`);
      // Every cell carries its own immutable provenance: measured by the
      // file's one method, by a verified tsc 7.0.2 executable, on exactly
      // this source (plus the measuring suffix) and this tsconfig.
      const receipt = measured.settings?.[cell.setting]?.receipt;
      if (receipt?.method !== JSON.stringify(expected.method))
        fail(`${key}: the reference cell was measured by another method than the file states`);
      if (
        !expected.method?.tscExeSha256 ||
        expected.method?.tscVersion !== `Version ${TYPESCRIPT_VERSION}`
      )
        fail(`${key}: the reference was not measured by a verified tsc ${TYPESCRIPT_VERSION}`);
      if (receipt?.tsconfigSha256 !== sha256Text(tsconfigText(setting)))
        fail(`${key}: the reference cell was measured with another tsconfig`);
      if (receipt?.sourceSha256 !== sha256Text(scenario.source + MEASURING_SUFFIX))
        fail(`${key}: the reference cell was measured on another source`);
    }
  }

  // Records against the plan.
  const opts = meta.options ?? {};
  const cellKeys = Object.keys(meta.scenarios ?? {});
  const expectedPlan = schedule(cellKeys, opts.arms ?? [], opts.repeat ?? 0, opts.warmup ?? 0);
  const plan = meta.plan ?? [];
  if (
    stable(plan) !==
    stable(expectedPlan.map((p) => `${p.key}|${p.arm}|${p.warmup ? "w" : "r"}${p.rep}`))
  ) {
    fail(
      "the recorded plan is not the counterbalanced schedule for the run's cells, arms and rounds",
    );
  }
  if ((opts.repeat ?? 0) % 2) fail(`--repeat ${opts.repeat} is odd: the arm order cannot balance`);
  for (const p of scheduleBalanceProblems(expectedPlan, opts.arms ?? [])) fail(`schedule: ${p}`);
  const invs = run.invocations ?? [];
  if (!invs.length) fail("zero records: the run holds no invocation");
  const seen = new Map();
  invs.forEach((inv, position) => {
    const entry = planEntry(inv);
    if (inv.index !== position)
      fail(`record ${entry} has index ${inv.index} at position ${position}`);
    if (seen.has(entry)) fail(`duplicate record ${entry}`);
    seen.set(entry, inv);
    if (plan[position] !== entry)
      fail(`record ${position} is ${entry}; the plan says ${plan[position] ?? "<nothing>"}`);
  });
  for (const entry of plan) if (!seen.has(entry)) fail(`missing record ${entry}`);
  const perCellArm = new Map();
  for (const key of Object.keys(meta.scenarios ?? {})) {
    for (const arm of opts.arms ?? []) perCellArm.set(`${key}|${arm}`, 0);
  }
  for (const inv of invs) {
    const k = `${inv.scenario}/${inv.setting}|${inv.arm}`;
    if (!perCellArm.has(k)) fail(`record for an unplanned cell ${k}`);
    else perCellArm.set(k, perCellArm.get(k) + 1);
  }
  for (const [k, n] of perCellArm) {
    if (n === 0) fail(`zero records for ${k}`);
    else if (n !== (opts.repeat ?? 0) + (opts.warmup ?? 0))
      fail(`${k} has ${n} records, not ${opts.repeat} measured + ${opts.warmup} warmup`);
  }

  // Each invocation.
  for (const inv of invs) {
    const id = planEntry(inv);
    if (inv.skipped) {
      const source = invs[inv.skipped.after];
      const valid =
        opts.skipAfterKill &&
        source &&
        source.index < inv.index &&
        source.warmup &&
        source.scenario === inv.scenario &&
        source.setting === inv.setting &&
        source.arm === inv.arm &&
        source.supervisor?.killedBy === "memory" &&
        invocationEnd(source, { budgetBytes: (opts.memMb ?? 0) * 1024 * 1024 }).kind === "killed";
      if (!valid)
        fail(
          `${id}: skipped without a warmup of the same scenario and arm whose engine exhausted the memory cap`,
        );
      continue;
    }
    const sup = inv.supervisor;
    for (const p of supervisorRecordProblems(sup)) fail(`${id}: ${p}`);
    if (!sup) continue;
    if (sup.containment !== "hard" && !(sup.containment === "sampled" && opts.allowSampled)) {
      fail(`${id}: containment ${sup.containment} without consent (--allow-sampled)`);
    }
    const cap = ((opts.memMb ?? 0) + (opts.infraMb ?? 0)) * 1024 * 1024;
    if (sup.memLimitBytes !== cap)
      fail(
        `${id}: containment cap ${sup.memLimitBytes} is not the run's budget plus allowance (${cap})`,
      );
    if (sup.timeoutMs !== opts.timeoutMs)
      fail(`${id}: deadline ${sup.timeoutMs} is not the run's ${opts.timeoutMs} ms`);
    const end = invocationEnd(inv, { budgetBytes: (opts.memMb ?? 0) * 1024 * 1024 });
    const expectedExit = ["killed", "observe-killed", "unattributed-kill"].includes(end.kind)
      ? sup.killedBy === "timeout"
        ? 124
        : 137
      : end.kind === "exited"
        ? sup.exitCode
        : 125;
    if (inv.supervisorExit !== expectedExit)
      fail(
        `${id}: supervisor exit ${inv.supervisorExit} disagrees with its record (${expectedExit})`,
      );
    if (end.kind === "harness-failure") {
      fail(`${id}: failed child: ${end.detail}`);
      continue;
    }
    const arm = ARMS[inv.arm];
    if (end.kind === "killed" || end.kind === "unattributed-kill") continue;
    if (end.kind === "observe-killed") {
      // The demand was measured and recorded before the kill: its record
      // must hold every demand field.
      for (const p of probeRecordProblems(inv.probe, {
        tool: arm.tool,
        warmRepeats: opts.warmRepeats,
        stage: "measured",
      }))
        fail(`${id}: ${p}`);
      continue;
    }
    if (arm.kind === "cli") {
      if (![0, 1, 2].includes(sup.exitCode))
        fail(`${id}: failed child: tsc exited ${sup.exitCode}`);
      else if (!parseCli(inv.cliStdout ?? "").complete)
        fail(`${id}: failed child: tsc printed no extended diagnostics`);
      continue;
    }
    if (sup.exitCode !== 0) {
      fail(`${id}: failed child: exit ${sup.exitCode}`);
      continue;
    }
    const r = inv.probe;
    if (!r) {
      fail(`${id}: failed child: no probe record (${inv.probeReadError})`);
      continue;
    }
    for (const p of probeRecordProblems(r, { tool: arm.tool, warmRepeats: opts.warmRepeats }))
      fail(`${id}: ${p}`);
    if (arm.tool === "verter") {
      if (r.instrumented !== (inv.arm === "verter-counted"))
        fail(`${id}: instrumentation flag ${r.instrumented} is wrong for ${inv.arm}`);
      if (r.observability !== (inv.arm === "verter-obs"))
        fail(`${id}: observability flag ${r.observability} is wrong for ${inv.arm}`);
      const exe = inv.arm === "verter-counted" ? bins.counted?.pinned : bins.probe?.pinned;
      if (inv.command?.[0] !== exe)
        fail(`${id}: ran ${inv.command?.[0]}, not the pinned probe ${exe}`);
    } else {
      if (r.tscExe !== ts.exe)
        fail(`${id}: the API ran ${r.tscExe}, not the verified tsc ${ts.exe}`);
      const dir = String(meta.scenarios?.[`${inv.scenario}/${inv.setting}`]?.dir ?? "").replace(
        /\\/g,
        "/",
      );
      const roots = (r.rootFiles ?? []).map((f) => f.toLowerCase());
      const want = [`${dir}/lib.bench.d.ts`, `${dir}/scenario.ts`].map((f) => f.toLowerCase());
      if (roots.length !== 2 || want.some((w) => !roots.includes(w)))
        fail(`${id}: tsc program roots ${JSON.stringify(r.rootFiles)} are not the scenario's`);
    }
    const answer = probeAnswer(inv, { budgetBytes: (opts.memMb ?? 0) * 1024 * 1024 });
    if (answer.alias && answer.alias !== "__Probe") fail(`${id}: answered ${answer.alias}`);
  }

  // Answers, classes and the summary, recomputed from the raw records.
  const recomputed = summarize(run, expected, scenarios);
  for (const cell of recomputed.cells) {
    for (const [arm, s] of Object.entries(cell.arms)) {
      const id = `${cell.key}|${arm}`;
      if (ARMS[arm].kind === "probe") {
        if (s.distinctAnswers > 1 || s.class === "inconsistent")
          fail(
            `${id}: inconsistent repetitions (${s.distinctAnswers} distinct answers, classes ${JSON.stringify(s.classes ?? s.class)})`,
          );
        if (arm === "tsc-api" && s.class === "inconsistent-with-reference") {
          for (const p of s.referenceProblems)
            fail(`${id}: wrong answer against the measured reference: ${p}`);
        }
        if (requireAllMatched && ARMS[arm].tool === "verter" && s.class !== "matched") {
          fail(`${id}: Verter's answer is ${s.class} (${s.detail}); --require-all-matched`);
        }
      } else if (s.distinctCodeSets > 1) {
        fail(
          `${id}: inconsistent repetitions (tsc -p reported ${s.distinctCodeSets} different diagnostic sets)`,
        );
      }
    }
    if (!cell.reference) warnings.push(`${cell.key}: no measured reference`);
    // Every completed measurement of both headline arms must read one metric.
    const metricSet = new Set([
      ...(cell.arms.verter?.memoryMetrics ?? []),
      ...(cell.arms["tsc-api"]?.memoryMetrics ?? []),
    ]);
    if (metricSet.size > 1)
      fail(`${cell.key}: the measurements' memory metrics differ (${[...metricSet].join(", ")})`);
    if (cell.headline) {
      for (const arm of ["verter", "tsc-api"]) {
        const s = cell.arms[arm];
        for (const m of ["coldMs", "firstTypeMs", "warmMs", "peakBytes", "retainedBytes"]) {
          if (s.metrics[m]?.n !== s.measured)
            fail(
              `${cell.key}|${arm}: ${m} has ${s.metrics[m]?.n ?? 0} of ${s.measured} measured values`,
            );
        }
      }
    }
  }
  if (!run.summary) fail("the run holds no summary");
  else if (stable(run.summary) !== stable(recomputed)) {
    const stored = new Map((run.summary.cells ?? []).map((c) => [c.key, c]));
    let reported = 0;
    for (const cell of recomputed.cells) {
      const s = stored.get(cell.key);
      if (stable(s) !== stable(cell) && reported++ < 20) {
        for (const arm of Object.keys(cell.arms)) {
          if (stable(s?.arms?.[arm]) !== stable(cell.arms[arm])) {
            fail(
              `${cell.key}|${arm}: the stored summary (class ${s?.arms?.[arm]?.class}) disagrees with its raw records (class ${cell.arms[arm].class})`,
            );
          }
        }
        if (stable(s?.headline) !== stable(cell.headline))
          fail(`${cell.key}: the stored comparison disagrees with its raw records`);
      }
    }
    if (!reported) fail("the stored summary disagrees with its raw records");
  }
  return { ok: failures.length === 0, failures, warnings };
}

/**
 * Re-read every invocation's raw files from disk and fail if they differ
 * from the copies embedded in results.json (tampering or a stale file).
 */
export function rawFileProblems(run) {
  const problems = [];
  for (const inv of run.invocations ?? []) {
    if (inv.skipped) continue;
    const id = planEntry(inv);
    try {
      const sup = JSON.parse(readFileSync(inv.supervisorOut, "utf8"));
      const { samples: _s, ...rest } = sup;
      const embedded = { ...inv.supervisor };
      delete embedded.samples;
      delete embedded.sampleCount;
      if (stable(rest) !== stable(embedded))
        problems.push(`${id}: the supervisor record on disk differs from results.json`);
    } catch (err) {
      problems.push(`${id}: cannot read ${inv.supervisorOut}: ${err.message}`);
    }
    if (inv.probeOut) {
      let marker = null;
      try {
        marker = JSON.parse(readFileSync(`${inv.probeOut}.phase`, "utf8"));
      } catch {
        marker = null;
      }
      if ((marker?.phase ?? null) !== (inv.phase ?? null))
        problems.push(`${id}: the phase marker on disk differs from results.json`);
    }
    if (inv.probeOut && inv.probe) {
      try {
        const probe = compactProbeRecord(JSON.parse(readFileSync(inv.probeOut, "utf8")));
        if (stable(probe) !== stable(inv.probe))
          problems.push(`${id}: the probe record on disk differs from results.json`);
      } catch (err) {
        problems.push(`${id}: cannot read ${inv.probeOut}: ${err.message}`);
      }
    }
  }
  return problems;
}

async function cli(argv) {
  const path = argv.find((a) => !a.startsWith("--"));
  if (!path) {
    console.error("usage: validate.mjs <results.json> [--require-all-matched]");
    return 2;
  }
  const run = JSON.parse(readFileSync(path, "utf8"));
  const expectedPath = join(HERE, "expected.json");
  const expected = JSON.parse(readFileSync(expectedPath, "utf8"));
  const failures = [];
  if (sha256File(expectedPath) !== run.meta?.expected?.sha256) {
    failures.push("the measured reference changed since the run (expected.json digest differs)");
  }
  failures.push(...rawFileProblems(run));
  const result = validateRun(run, expected, allScenarios(), {
    requireAllMatched: argv.includes("--require-all-matched"),
  });
  failures.push(...result.failures);
  for (const w of result.warnings) console.log(`warning: ${w}`);
  for (const f of failures) console.log(`FAIL: ${f}`);
  console.log(failures.length ? `validation FAILED (${failures.length})` : "validation passed");
  return failures.length ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  cli(process.argv.slice(2)).then((code) => process.exit(code));
}
