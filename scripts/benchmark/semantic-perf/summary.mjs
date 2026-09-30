// The per-(scenario, setting) summary, derived only from raw invocation
// records and the measured reference. The validator recomputes it and fails
// a run whose stored summary differs.

import {
  ARMS,
  classifyVerterAnswer,
  invocationEnd,
  parseCli,
  probeAnswer,
  probeMetrics,
  stats,
  tscAnswerStatus,
  verdict,
} from "./analyze.mjs";
import { canonicalDigest } from "./canonical.mjs";
import { interpretMeasurement } from "./reference.mjs";

const referenceCache = new Map();
/** The interpreted reference of one (scenario, setting), or null. */
export function referenceFor(expected, scenarioId, settingId) {
  const raw = expected.scenarios?.[scenarioId]?.settings?.[settingId];
  if (!raw) return null;
  const key = `${expected.method?.measuringSuffixSha256}|${scenarioId}|${settingId}`;
  if (!referenceCache.has(key) || referenceCache.get(key).raw !== raw) {
    let answer;
    try {
      answer = interpretMeasurement(raw);
    } catch (err) {
      answer = {
        gap: `uninterpretable reference: ${err.message}`,
        unmeasurable: "uninterpretable",
        codes: raw.codes ?? [],
      };
    }
    referenceCache.set(key, { raw, answer });
  }
  return referenceCache.get(key).answer;
}

const digestCache = new Map();
function digestOf(text) {
  if (!digestCache.has(text)) {
    let digest = null;
    try {
      digest = canonicalDigest(text);
    } catch {
      digest = null;
    }
    digestCache.set(text, digest);
  }
  return digestCache.get(text);
}
/** The digest of a scenario's probe expression itself (an unevaluated answer). */
export const probeDigest = (scenario) => (scenario?.probe ? digestOf(scenario.probe) : null);
/** The digest of a scenario's constructed answer past tsc's limit, or null. */
export const beyondDigest = (scenario) => (scenario?.beyond ? digestOf(scenario.beyond) : null);

const PROBE_METRICS = [
  "engineStartMs",
  "setupMs",
  "initMs",
  "coldMs",
  "firstTypeMs",
  "warmMs",
  "observeMs",
  "teardownMs",
  "peakBytes",
  "retainedBytes",
  "observePeakBytes",
  "cpuMs",
];

function probeArmSummary(arm, invs, ctx) {
  const measured = invs.filter((i) => !i.warmup);
  const answers = invs.map((i) => probeAnswer(i, { budgetBytes: ctx.budgetBytes }));
  const digests = [
    ...new Set(answers.map((a) => a.digest?.sha256 ?? `<${a.end.kind}:${a.outcome?.kind ?? ""}>`)),
  ];
  const out = {
    invocations: invs.length,
    measured: measured.length,
    answerDigest: answers.find((a) => a.digest)?.digest ?? null,
    distinctAnswers: digests.length,
    ends: [...new Set(answers.map((a) => a.end.kind))],
  };
  if (ARMS[arm].tool === "verter") {
    const classes = answers.map((a) => classifyVerterAnswer(a, ctx));
    out.classes = [...new Set(classes.map((c) => c.class))];
    out.class = out.classes.length === 1 ? out.classes[0] : "inconsistent";
    out.detail = classes[0]?.detail ?? "";
  } else {
    const statuses = answers.map((a) =>
      tscAnswerStatus(a, ctx.reference, ctx.beyond, ctx.budgetBytes),
    );
    out.referenceProblems = [...new Set(statuses.filter((s) => s.problem).map((s) => s.problem))];
    const kinds = [...new Set(statuses.map((s) => s.status))];
    out.class = out.referenceProblems.length
      ? "inconsistent-with-reference"
      : kinds.length === 1
        ? {
            killed: "killed",
            reference: "reference",
            "by-construction": "reference-by-construction",
            unverified: "unverified",
          }[kinds[0]]
        : "inconsistent";
    out.detail = statuses.find((s) => s.detail)?.detail ?? "";
    out.errorType = answers.find((a) => a.digest)?.errorType ?? null;
  }
  // Statistics only over invocations whose demand completed; the validator
  // fails a run where a completed invocation lacks any of them.
  const completed = measured.filter((i) =>
    ["exited", "observe-killed"].includes(
      probeAnswer(i, { budgetBytes: ctx.budgetBytes }).end.kind,
    ),
  );
  const metrics = completed.map(probeMetrics);
  out.completedMeasured = completed.length;
  out.metrics = Object.fromEntries(
    PROBE_METRICS.map((m) => [m, stats(metrics.map((x) => x?.[m]))]),
  );
  // Every completed measurement's memory metric (the validator requires one
  // metric across both headline arms).
  out.memoryMetrics = [...new Set(metrics.map((m) => m?.memoryMetric ?? "<none>"))];
  out.memoryMetric = out.memoryMetrics.length === 1 ? out.memoryMetrics[0] : null;
  if (ARMS[arm].tool === "tsc") {
    for (const m of ["spawnMs", "setupRoundTripMs", "coldRoundTripMs", "warmRoundTripMs"])
      out.metrics[m] = stats(metrics.map((x) => x?.[m]));
  }
  if (arm === "verter-counted") {
    out.coldAllocations = stats(metrics.map((x) => x?.allocations?.[0]));
    out.coldAllocatedBytes = stats(metrics.map((x) => x?.allocations?.[1]));
  }
  if (arm === "verter") out.retention = metrics[0]?.retention ?? null;
  out.invocationWallMs = stats(measured.map((i) => i.supervisor?.wallMs));
  out.invocationTreePeakBytes = stats(measured.map((i) => i.supervisor?.peakBytes));
  return out;
}

function cliArmSummary(invs, budgetBytes) {
  const measured = invs.filter((i) => !i.warmup);
  const ends = invs.map((i) => invocationEnd(i, { budgetBytes }));
  const parsed = invs.map((i, k) =>
    ends[k].kind === "exited" ? parseCli(i.cliStdout ?? "") : null,
  );
  const codeSets = [
    ...new Set(parsed.map((p, k) => (p ? p.codes.join(",") : `<${ends[k].kind}>`))),
  ];
  const completed = measured.filter((i) => invocationEnd(i, { budgetBytes }).kind === "exited");
  const measuredParsed = completed.map((i) => parseCli(i.cliStdout ?? ""));
  const overBudget = completed.filter(
    (i) => typeof i.supervisor?.peakBytes === "number" && i.supervisor.peakBytes > budgetBytes,
  ).length;
  const killed = invs.filter((i) =>
    ["killed", "unattributed-kill"].includes(invocationEnd(i, { budgetBytes }).kind),
  );
  return {
    invocations: invs.length,
    measured: measured.length,
    status:
      killed.length === invs.length
        ? `killed (${invocationEnd(killed[0], { budgetBytes }).detail})`
        : killed.length
          ? "inconsistent"
          : overBudget
            ? "over the engine budget"
            : "completed",
    ends: [...new Set(ends.map((e) => e.kind))],
    codes: parsed.find(Boolean)?.codes ?? null,
    distinctCodeSets: codeSets.length,
    wallMs: stats(completed.map((i) => i.supervisor?.wallMs)),
    terminationMs: stats(
      measured
        .filter((i) => invocationEnd(i, { budgetBytes }).kind === "killed" && !i.skipped)
        .map((i) => i.supervisor?.wallMs),
    ),
    peakBytes: stats(completed.map((i) => i.supervisor?.peakBytes)),
    peakMetric: completed[0]?.supervisor?.peakMetric ?? null,
    cpuMs: stats(
      completed.map((i) => (i.supervisor?.cpuUserMs ?? NaN) + (i.supervisor?.cpuKernelMs ?? NaN)),
    ),
    tscMemoryUsedBytes: stats(measuredParsed.map((p) => p.memoryUsedBytes)),
    tscCheckMs: stats(measuredParsed.map((p) => p.checkMs)),
    tscTotalMs: stats(measuredParsed.map((p) => p.totalMs)),
  };
}

/**
 * The resolution below which a timing difference is not claimed, from a
 * calibration rather than the workload: every tsc probe times a dedicated
 * series of 20 trivial warm requests after its measurement.
 *
 * - If any calibration request reads 0 ms, the server clock is coarse (a
 *   request cannot take no time): every reading is a whole number of clock
 *   quanta, so the quantum is the smallest positive tsc server time of the
 *   run, whichever request produced it (about half a millisecond on Windows).
 * - Otherwise the clock resolves the trivial request, and the quantum is at
 *   most the smallest calibration time.
 *
 * Verter's timer is in microseconds. One request: at least 1 ms and two
 * quanta (a difference of two readings); a sum of three separately timed
 * requests (first type handle): at least 2 ms and four quanta. This is a
 * descriptive threshold, not a confidence bound.
 */
export function timerResolution(run) {
  const calibration = [];
  const server = [];
  for (const inv of run.invocations ?? []) {
    const r = inv.probe;
    if (inv.arm !== "tsc-api" || r?.tool !== "tsc") continue;
    for (const t of r.calibration?.serverMs ?? [])
      if (typeof t === "number" && Number.isFinite(t)) calibration.push(t);
    const p = r.probes?.[0];
    for (const t of [
      r.phases?.engineStartMs,
      r.phases?.setupMs,
      r.init?.serverMs,
      p?.cold?.serverMs,
      ...(p?.warm ?? []).map((w) => w.serverMs),
      ...calibration,
    ]) {
      if (typeof t === "number" && Number.isFinite(t) && t > 0) server.push(t);
    }
  }
  const zeroShare = calibration.length
    ? calibration.filter((t) => t === 0).length / calibration.length
    : null;
  const coarse = zeroShare !== null && zeroShare > 0;
  const pool = coarse ? server : calibration.filter((t) => t > 0);
  const quantum = pool.length ? Math.min(...pool) : null;
  return {
    tscQuantumMs: quantum,
    clock: calibration.length === 0 ? "uncalibrated" : coarse ? "coarse" : "fine",
    calibrationSamples: calibration.length,
    calibrationZeroShare: zeroShare,
    single: Math.max(1, 2 * (quantum ?? 0)),
    sum: Math.max(2, 4 * (quantum ?? 0)),
  };
}

/** The fixed floors of the resolution (the report states them). */
export const TIME_RESOLUTION_MS = { single: 1, sum: 2 };

function comparison(verterInvs, tscInvs, resolution) {
  const v = verterInvs.filter((i) => !i.warmup).map(probeMetrics);
  const t = tscInvs.filter((i) => !i.warmup).map(probeMetrics);
  const pick = (xs, m) => xs.map((x) => x?.[m]);
  const out = {};
  for (const m of ["coldMs", "warmMs", "setupMs", "initMs"])
    out[m] = verdict(pick(v, m), pick(t, m), resolution.single);
  out.firstTypeMs = verdict(pick(v, "firstTypeMs"), pick(t, "firstTypeMs"), resolution.sum);
  for (const m of ["peakBytes", "retainedBytes"]) out[m] = verdict(pick(v, m), pick(t, m), 0);
  return out;
}

/** Summarise a run against the measured reference. */
export function summarize(run, expected, scenarios) {
  const byId = new Map(scenarios.map((s) => [s.id, s]));
  const budgetBytes = (run.meta?.options?.memMb ?? 0) * 1024 * 1024;
  const resolution = timerResolution(run);
  const groups = new Map();
  for (const inv of run.invocations) {
    const key = `${inv.scenario}/${inv.setting}`;
    if (!groups.has(key)) groups.set(key, {});
    const arms = groups.get(key);
    (arms[inv.arm] ??= []).push(inv);
  }
  const cells = [];
  for (const key of Object.keys(run.meta.scenarios)) {
    const meta = run.meta.scenarios[key];
    const scenario = byId.get(meta.id);
    const measured = referenceFor(expected, meta.id, meta.setting);
    const armInvs = groups.get(key) ?? {};
    const base = { beyond: beyondDigest(scenario), probe: probeDigest(scenario), budgetBytes };
    const arms = {};
    // The tsc arm first: when the measurement holds no answer but the API
    // answered with the constructed one, that is the reference the Verter
    // arms are classified against; and a Verter answer past tsc's limit
    // counts as beyond tsc only where the tsc arm was killed on the demand.
    if (armInvs["tsc-api"])
      arms["tsc-api"] = probeArmSummary("tsc-api", armInvs["tsc-api"], {
        ...base,
        reference: measured,
      });
    const reference =
      arms["tsc-api"]?.class === "reference-by-construction"
        ? {
            digest: beyondDigest(scenario),
            errorAny: false,
            codes: measured?.codes ?? [],
            byConstruction: true,
            measuredGap: measured?.gap ?? null,
          }
        : measured;
    const ctx = { ...base, reference, tscKilled: arms["tsc-api"]?.class === "killed" };
    for (const [arm, invs] of Object.entries(armInvs)) {
      if (arm === "tsc-api") continue;
      arms[arm] =
        ARMS[arm].kind === "cli"
          ? cliArmSummary(invs, budgetBytes)
          : probeArmSummary(arm, invs, ctx);
    }
    const comparableTsc = ["reference", "reference-by-construction"].includes(
      arms["tsc-api"]?.class,
    );
    const headline =
      arms.verter?.class === "matched" && comparableTsc
        ? comparison(armInvs.verter, armInvs["tsc-api"], resolution)
        : null;
    const obsCost =
      armInvs.verter && armInvs["verter-obs"]
        ? {
            coldMs: verdict(
              armInvs.verter.filter((i) => !i.warmup).map((i) => probeMetrics(i)?.coldMs),
              armInvs["verter-obs"].filter((i) => !i.warmup).map((i) => probeMetrics(i)?.coldMs),
              resolution.single,
            ),
          }
        : null;
    cells.push({
      key,
      scenario: meta.id,
      setting: meta.setting,
      family: meta.family,
      note: meta.note,
      reference: reference
        ? {
            digest: reference.digest ?? null,
            errorAny: reference.errorAny ?? null,
            codes: reference.codes ?? [],
            gap: reference.gap ?? null,
            killed: reference.killed ?? null,
            byConstruction: reference.byConstruction ?? false,
            measuredGap: reference.measuredGap ?? null,
          }
        : null,
      arms,
      headline,
      obsCost,
    });
  }
  const counts = {};
  for (const cell of cells) {
    const c = cell.arms.verter?.class ?? "not-run";
    counts[c] = (counts[c] ?? 0) + 1;
  }
  return { cells, verterClassCounts: counts, resolution };
}
