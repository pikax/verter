// The per-(scenario, setting) summary, derived only from raw invocation
// records and the measured reference. The validator recomputes it and fails
// a run whose stored summary differs.

import { ARMS, classifyVerterAnswer, invocationEnd, parseCli, probeAnswer, probeMetrics, stats, tscAnswerProblem, verdict } from "./analyze.mjs";
import { canonicalDigest } from "./canonical.mjs";

/** The reference entry of one (scenario, setting), or null. */
export function referenceFor(expected, scenarioId, settingId) {
  return expected.scenarios?.[scenarioId]?.settings?.[settingId] ?? null;
}

const beyondCache = new Map();
const probeCache = new Map();
/** The digest of a scenario's probe expression itself (an unevaluated answer). */
export function probeDigest(scenario) {
  if (!scenario?.probe) return null;
  if (!probeCache.has(scenario.id)) {
    let digest = null;
    try {
      digest = canonicalDigest(scenario.probe);
    } catch {
      digest = null;
    }
    probeCache.set(scenario.id, digest);
  }
  return probeCache.get(scenario.id);
}

/** The digest of a scenario's constructed beyond-limit answer, or null. */
export function beyondDigest(scenario) {
  if (!scenario?.beyond) return null;
  if (!beyondCache.has(scenario.id)) beyondCache.set(scenario.id, canonicalDigest(scenario.beyond));
  return beyondCache.get(scenario.id);
}

function metricStats(invs, pick) {
  return stats(invs.map(pick));
}

const PROBE_METRICS = ["setupMs", "initMs", "coldMs", "firstAnswerMs", "warmMs", "observeMs", "teardownMs", "peakBytes", "retainedBytes", "cpuMs"];

function probeArmSummary(arm, invs, reference, scenario) {
  const measured = invs.filter((i) => !i.warmup);
  const answers = invs.map(probeAnswer);
  const digests = [...new Set(answers.map((a) => a.digest?.sha256 ?? `<${a.end.kind}:${a.outcome?.kind ?? ""}>`))];
  const out = {
    invocations: invs.length,
    measured: measured.length,
    answerDigest: answers[0]?.digest ?? null,
    distinctAnswers: digests.length,
    ends: [...new Set(answers.map((a) => a.end.kind))],
  };
  if (ARMS[arm].tool === "verter") {
    const classes = answers.map((a) => classifyVerterAnswer(a, reference, beyondDigest(scenario), probeDigest(scenario)));
    out.classes = [...new Set(classes.map((c) => c.class))];
    out.class = out.classes.length === 1 ? out.classes[0] : "inconsistent";
    out.detail = classes[0]?.detail ?? "";
  } else {
    const problems = answers.map((a) => tscAnswerProblem(a, reference));
    out.referenceProblems = [...new Set(problems.filter(Boolean))];
    const killed = answers.every((a) => a.end.kind === "killed");
    out.class = killed ? "killed" : out.referenceProblems.length ? "inconsistent-with-reference" : "reference";
    out.errorType = answers[0]?.errorType ?? null;
  }
  const metrics = measured.map(probeMetrics);
  out.metrics = Object.fromEntries(PROBE_METRICS.map((m) => [m, metricStats(metrics, (x) => x?.[m])]));
  out.memoryMetric = metrics.find((m) => m?.memoryMetric)?.memoryMetric ?? null;
  if (ARMS[arm].tool === "tsc") {
    out.metrics.spawnMs = metricStats(metrics, (x) => x?.spawnMs);
    out.metrics.coldRoundTripMs = metricStats(metrics, (x) => x?.coldRoundTripMs);
    out.metrics.warmRoundTripMs = metricStats(metrics, (x) => x?.warmRoundTripMs);
    out.diagnosticCodes = metrics[0]?.diagnosticCodes ?? null;
  }
  if (arm === "verter-counted") {
    out.coldAllocations = metricStats(metrics, (x) => x?.allocations?.[0]);
    out.coldAllocatedBytes = metricStats(metrics, (x) => x?.allocations?.[1]);
  }
  if (arm === "verter") out.retention = metrics[0]?.retention ?? null;
  out.invocationWallMs = stats(measured.map((i) => i.supervisor?.wallMs));
  out.invocationTreePeakBytes = stats(measured.map((i) => i.supervisor?.peakBytes));
  return out;
}

function cliArmSummary(invs) {
  const measured = invs.filter((i) => !i.warmup);
  const parsed = invs.map((i) => (invocationEnd(i).kind === "exited" ? parseCli(i.cliStdout ?? "") : null));
  const codes = [...new Set(parsed.map((p) => (p ? p.codes.join(",") : "<none>")))];
  const measuredParsed = measured.map((i) => (invocationEnd(i).kind === "exited" ? parseCli(i.cliStdout ?? "") : null));
  return {
    invocations: invs.length,
    measured: measured.length,
    ends: [...new Set(invs.map((i) => invocationEnd(i).kind))],
    codes: parsed[0]?.codes ?? null,
    distinctCodeSets: codes.length,
    wallMs: stats(measured.map((i) => i.supervisor?.wallMs)),
    peakBytes: stats(measured.map((i) => i.supervisor?.peakBytes)),
    peakMetric: measured[0]?.supervisor?.peakMetric ?? null,
    cpuMs: stats(measured.map((i) => (i.supervisor?.cpuUserMs ?? NaN) + (i.supervisor?.cpuKernelMs ?? NaN))),
    tscMemoryUsedBytes: stats(measuredParsed.map((p) => p?.memoryUsedBytes)),
    tscCheckMs: stats(measuredParsed.map((p) => p?.checkMs)),
    tscTotalMs: stats(measuredParsed.map((p) => p?.totalMs)),
  };
}

// The resolution below which a timing difference is not claimed: Verter's
// timer is in microseconds; tsc's server-side timer is coarse on Windows.
const TIME_RESOLUTION_MS = 1;

function comparison(verterInvs, tscInvs) {
  const v = verterInvs.filter((i) => !i.warmup).map(probeMetrics);
  const t = tscInvs.filter((i) => !i.warmup).map(probeMetrics);
  const pick = (xs, m) => xs.map((x) => x?.[m]);
  const out = {};
  for (const m of ["coldMs", "warmMs", "firstAnswerMs", "setupMs", "initMs"]) {
    out[m] = verdict(pick(v, m), pick(t, m), TIME_RESOLUTION_MS);
  }
  for (const m of ["peakBytes", "retainedBytes"]) out[m] = verdict(pick(v, m), pick(t, m), 0);
  return out;
}

/** Summarise a run against the measured reference. */
export function summarize(run, expected, scenarios) {
  const byId = new Map(scenarios.map((s) => [s.id, s]));
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
    const reference = referenceFor(expected, meta.id, meta.setting);
    const arms = {};
    for (const [arm, invs] of Object.entries(groups.get(key) ?? {})) {
      arms[arm] = ARMS[arm].kind === "cli" ? cliArmSummary(invs) : probeArmSummary(arm, invs, reference, scenario);
    }
    const headline =
      arms.verter?.class === "matched" && arms["tsc-api"]?.class === "reference"
        ? comparison(groups.get(key).verter, groups.get(key)["tsc-api"])
        : null;
    const obsCost =
      arms.verter && arms["verter-obs"]
        ? {
            coldMs: verdict(
              groups.get(key).verter.filter((i) => !i.warmup).map((i) => probeMetrics(i)?.coldMs),
              groups.get(key)["verter-obs"].filter((i) => !i.warmup).map((i) => probeMetrics(i)?.coldMs),
              TIME_RESOLUTION_MS,
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
        ? { digest: reference.digest ?? null, errorAny: reference.errorAny ?? null, codes: reference.codes ?? [], killed: reference.killed ?? null }
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
  return { cells, verterClassCounts: counts };
}
