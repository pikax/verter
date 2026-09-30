// Answer extraction, record checks, classification and statistics for the
// semantic benchmark. Pure functions over raw invocation records: the
// harness uses them to write its summary and the validator re-derives every
// summary field from the raw records with them, so a summary that disagrees
// with its own raw records fails validation.

import { createHash } from "node:crypto";

import { canonicalDigest } from "./canonical.mjs";
import { RESOURCE_CODES } from "./reference.mjs";

export { RESOURCE_CODES };

/** The arms, what each measures and whether it enters the head-to-head. */
export const ARMS = {
  verter: { tool: "verter", kind: "probe", headline: true, label: "Verter (production defaults)" },
  "tsc-api": { tool: "tsc", kind: "probe", headline: true, label: "tsc 7.0.2 native API" },
  "verter-obs": { tool: "verter", kind: "probe", headline: false, label: "Verter, observability on (labelled; not compared)" },
  "verter-counted": { tool: "verter", kind: "probe", headline: false, label: "Verter, counting allocator (instrumented; not compared)" },
  "tsc-cli": { tool: "tsc", kind: "cli", headline: false, label: "tsc -p, default (parallel) checkers (whole program; reference)" },
  "tsc-cli-1": { tool: "tsc", kind: "cli", headline: false, label: "tsc -p --singleThreaded (whole program; reference)" },
};

export const DEFAULT_ARMS = Object.keys(ARMS);

/** Verter classes. */
export const CLASSES = ["killed", "error", "refusal", "unverified", "partial", "mismatch", "no-reference", "beyond-tsc", "matched"];

/** The phases a probe's demand runs in (a kill in one of them is the engine's). */
export const DEMAND_PHASES = ["spawn", "engine-start", "setup", "init", "cold", "warm", "stats"];

const finite = (v) => typeof v === "number" && Number.isFinite(v) && v >= 0;

/** How an invocation ended. */
export function invocationEnd(inv) {
  if (inv.skipped) return { kind: "killed", detail: "memory (skipped after a warmup killed at the cap)" };
  const rec = inv.supervisor;
  if (!rec) return { kind: "harness-failure", detail: inv.supervisorReadError ?? "no supervisor record" };
  if (!rec.launched) return { kind: "harness-failure", detail: `not launched: ${(rec.errors ?? []).join("; ")}` };
  if (rec.killedBy === "memory" || rec.killedBy === "timeout") {
    const phase = inv.phase ?? null;
    // Stopped after the demand was measured and recorded: the answer was
    // being observed, which is the benchmark's machinery, not the engine.
    if (inv.probe?.stage === "measured" && phase && !DEMAND_PHASES.includes(phase)) {
      return { kind: "observe-killed", detail: `${rec.killedBy} during ${phase}` };
    }
    return { kind: "killed", detail: phase ? `${rec.killedBy} during ${phase}` : rec.killedBy };
  }
  if (rec.killedBy) return { kind: "harness-failure", detail: `killed by ${rec.killedBy}` };
  if ((rec.errors ?? []).length) return { kind: "harness-failure", detail: rec.errors.join("; ") };
  return { kind: "exited", exitCode: rec.exitCode };
}

/**
 * Problems with a probe record that must hold for its numbers to be read:
 * every phase and request time present, finite and non-negative, every warm
 * repeat answered with the cold answer, the statistics present and clean.
 * `stage` is the stage the record must have reached.
 */
export function probeRecordProblems(record, { tool, warmRepeats, stage = "complete" }) {
  const p = [];
  const need = (cond, what) => {
    if (!cond) p.push(what);
  };
  need(record?.schema === 2, `record schema ${record?.schema} is not 2`);
  need(record?.tool === tool, `record from ${record?.tool}, not ${tool}`);
  if (stage === "complete") need(record?.stage === "complete", `record stage ${record?.stage}, not complete`);
  else need(record?.stage === "measured" || record?.stage === "complete", `record stage ${record?.stage}`);
  const probe = record?.probes?.[0];
  need(record?.probes?.length === 1 && probe?.alias === "__Probe", `expected exactly one __Probe record, got ${record?.probes?.length ?? 0}`);
  if (!probe) return p;
  const warm = probe.warm ?? [];
  need(warm.length === warmRepeats, `${warm.length} warm repeats, not ${warmRepeats}`);
  if (tool === "verter") {
    const ph = record.phases ?? {};
    need(finite(ph.engineStart) && finite(ph.setup) && finite(ph.init), "a phase time is missing or invalid");
    need(record.init?.outcome?.kind === "value" && finite(record.init?.micros), "the init request did not answer");
    need(finite(probe.cold?.micros), "the cold time is missing or invalid");
    need(warm.every((w) => finite(w.micros)), "a warm time is missing or invalid");
    need(finite(record.afterRequests?.peakBytes) && finite(record.afterRequests?.currentBytes), "no engine statistics after the requests");
    if (record.stage === "complete") {
      need(finite(ph.teardown), "the teardown time is missing");
      need(finite(probe.observeMicros), "the observe time is missing");
      need(finite(record.afterObserve?.peakBytes), "no statistics after observation");
    }
  } else {
    const ph = record.phases ?? {};
    need(finite(ph.spawnMs) && finite(ph.engineStartMs) && finite(ph.setupMs) && finite(ph.setupRoundTripMs), "a phase time is missing or invalid");
    need(record.init?.outcome?.kind === "value" && finite(record.init?.serverMs) && finite(record.init?.roundTripMs), "the init request did not answer");
    need(finite(probe.cold?.serverMs) && finite(probe.cold?.roundTripMs), "the cold time is missing or invalid");
    need(warm.every((w) => finite(w.serverMs) && finite(w.roundTripMs)), "a warm time is missing or invalid");
    need(finite(record.serverAfterRequests?.peakBytes) && finite(record.serverAfterRequests?.currentBytes), "no engine statistics after the requests");
    if (record.stage === "complete") {
      need(finite(ph.teardownMs), "the teardown time is missing");
      need(finite(probe.observeMs), "the observe time is missing");
      need(finite(record.serverAfterObserve?.peakBytes), "no statistics after observation");
    }
  }
  need((record.statsErrors ?? []).length === 0, `statistics errors: ${(record.statsErrors ?? []).join("; ")}`);
  if (record.stage === "complete" && probe.cold?.outcome?.kind === "value") {
    need(warm.every((w) => w.outcome?.kind === "value"), "a warm repeat did not answer");
    need(warm.every((w) => w.sameAnswerAsCold === true), "a warm repeat answered differently from the cold request");
  }
  return p;
}

/** The observed answer of one probe invocation (arm kind "probe"). */
export function probeAnswer(inv) {
  let end = invocationEnd(inv);
  if (end.kind === "exited" && end.exitCode !== 0) end = { kind: "child-failure", detail: `exit ${end.exitCode}` };
  if (end.kind !== "exited" && end.kind !== "observe-killed") return { end };
  const result = inv.probe;
  if (!result) return { end: { kind: "child-failure", detail: inv.probeReadError ?? "no probe record" } };
  const probe = result.probes?.[0];
  if (!probe || result.probes.length !== 1) {
    return { end: { kind: "child-failure", detail: `expected exactly one probe record, got ${result.probes?.length ?? 0}` } };
  }
  const obs = probe.observation ?? {};
  let digest = null;
  let canonicalError = null;
  if (obs.textElided) {
    digest = obs.canonical ?? null;
    canonicalError = obs.canonicalError ?? null;
  } else if (typeof obs.text === "string") {
    try {
      digest = canonicalDigest(obs.text);
    } catch (err) {
      canonicalError = String(err.message ?? err);
    }
  }
  const stats = result.tool === "verter" ? result.afterRequests : result.serverAfterRequests;
  return {
    end,
    alias: probe.alias,
    outcome: probe.cold?.outcome ?? { kind: "missing" },
    warmKinds: (probe.warm ?? []).map((w) => w.outcome?.kind),
    warmSame: (probe.warm ?? []).map((w) => w.sameAnswerAsCold),
    observed: Boolean(probe.observation),
    digest,
    canonicalError,
    observeError: probe.observation ? (obs.error ?? null) : "the answer was not observed",
    errorType: obs.errorType ?? null,
    unknownLeaves: obs.unknownLeaves ?? 0,
    unknownSamples: obs.unknownSamples ?? [],
    shape: obs.shape ?? null,
    enginePeakBytes: stats?.peakBytes ?? null,
  };
}

function isBudgetFault(detail) {
  return /budget|limit|exhaust|depth/i.test(detail ?? "");
}

/** Whether a reference holds tsc's answer. */
export function referenceHasAnswer(reference) {
  return Boolean(reference && !reference.gap && reference.digest);
}

const overBudget = (answer, budgetBytes) =>
  typeof budgetBytes === "number" && typeof answer.enginePeakBytes === "number" && answer.enginePeakBytes > budgetBytes;

const mib = (bytes) => `${(bytes / 1048576).toFixed(0)} MiB`;

/**
 * Classify one Verter answer. `ctx`: `reference` (the interpreted tsc
 * answer), `beyond` / `probe` (digests of the scenario's constructed answer
 * and of its probe expression), `budgetBytes` (the engine budget),
 * `tscKilled` (the tsc arm was killed on this demand).
 */
export function classifyVerterAnswer(answer, ctx = {}) {
  const { reference = null, beyond = null, probe = null, budgetBytes = null, tscKilled = false } = ctx;
  const end = answer.end;
  if (end.kind === "killed") return { class: "killed", detail: end.detail };
  if (end.kind !== "exited" && end.kind !== "observe-killed") return { class: "error", detail: end.detail };
  if (answer.outcome.kind === "fault") {
    return isBudgetFault(answer.outcome.detail)
      ? { class: "refusal", detail: answer.outcome.detail }
      : { class: "error", detail: answer.outcome.detail };
  }
  if (answer.outcome.kind !== "value") return { class: "error", detail: `request outcome ${answer.outcome.kind}` };
  if (answer.warmKinds.some((k) => k !== "value")) return { class: "error", detail: "a warm repeat did not answer" };
  if (overBudget(answer, budgetBytes)) {
    return { class: "killed", detail: `the engine's peak ${mib(answer.enginePeakBytes)} exceeds the ${mib(budgetBytes)} budget` };
  }
  if (end.kind === "observe-killed") return { class: "unverified", detail: `stopped while its answer was observed (${end.detail})` };
  if (answer.warmSame.some((s) => s !== true)) return { class: "error", detail: "a warm repeat answered differently" };
  if (answer.observeError || !answer.digest) {
    return { class: "partial", detail: answer.observeError ?? answer.canonicalError ?? "no printable answer" };
  }
  if (answer.unknownLeaves > 0) {
    return { class: "partial", detail: `the answer holds ${answer.unknownLeaves} unmaterialised leaf/leaves (${answer.unknownSamples.join(", ")})` };
  }
  if (answer.shape === "conditional") return { class: "partial", detail: "the answer is an unevaluated conditional" };
  if (!referenceHasAnswer(reference)) {
    // Beyond tsc only where tsc's exhaustion is established on the demand
    // itself: the measuring program ran out and so did the API arm.
    if (reference?.killed && tscKilled && beyond && answer.digest.sha256 === beyond.sha256) {
      return { class: "beyond-tsc", detail: "tsc exhausts the cap on this demand; Verter's answer is the constructed one" };
    }
    return { class: "no-reference", detail: reference?.gap ?? "no measured reference" };
  }
  const limitCodes = reference.codes.filter((c) => RESOURCE_CODES.includes(c));
  if (!reference.errorAny && answer.digest.sha256 === reference.digest.sha256) {
    return { class: "matched", detail: limitCodes.length ? "equal to tsc's answer at its resource limit" : "" };
  }
  if (limitCodes.length && beyond && answer.digest.sha256 === beyond.sha256) {
    return { class: "beyond-tsc", detail: `tsc stops with TS${limitCodes.join("/TS")}` };
  }
  if (probe && answer.digest.sha256 === probe.sha256) return { class: "partial", detail: "the answer is the probe expression, unevaluated" };
  return { class: "mismatch", detail: `answered ${answer.digest.preview}, tsc ${reference.errorAny ? "error-any" : reference.digest.preview}` };
}

/**
 * The tsc arm's standing for one invocation:
 *   "killed"          the supervisor killed it during the demand, or its
 *                     engine exceeded the budget (a valid observation);
 *   "unverified"      stopped while observing, or its answer cannot be
 *                     checked (no reference and no constructed answer);
 *   "reference"       it reproduces the measured reference;
 *   "by-construction" the measurement has no answer, but the API's answer is
 *                     the scenario's constructed one;
 *   "problem"         it contradicts the reference or failed (fails validation).
 */
export function tscAnswerStatus(answer, reference, beyond, budgetBytes = null) {
  const end = answer.end;
  if (end.kind === "killed") return { status: "killed", detail: end.detail };
  if (end.kind !== "exited" && end.kind !== "observe-killed") return { status: "problem", problem: `tsc invocation failed: ${end.detail}` };
  if (answer.outcome.kind !== "value") return { status: "problem", problem: `tsc request outcome ${answer.outcome.kind} ${answer.outcome.detail ?? ""}` };
  if (answer.warmKinds.some((k) => k !== "value")) return { status: "problem", problem: "a warm tsc request did not answer" };
  if (overBudget(answer, budgetBytes)) {
    return { status: "killed", detail: `the engine's peak ${mib(answer.enginePeakBytes)} exceeds the ${mib(budgetBytes)} budget` };
  }
  if (end.kind === "observe-killed") return { status: "unverified", detail: `stopped while its answer was observed (${end.detail})` };
  if (answer.warmSame.some((s) => s !== true)) return { status: "problem", problem: "a warm tsc request answered differently" };
  if (!reference) return { status: "problem", problem: "no measured reference for this scenario and setting" };
  if (!answer.digest) return { status: "problem", problem: `tsc printed no answer: ${answer.observeError ?? answer.canonicalError}` };
  if (!referenceHasAnswer(reference)) {
    if (!answer.errorType && beyond && answer.digest.sha256 === beyond.sha256) return { status: "by-construction" };
    return { status: "unverified", detail: reference.gap };
  }
  if (answer.digest.sha256 !== reference.digest.sha256) {
    return { status: "problem", problem: `tsc answered ${answer.digest.preview}; the measured reference is ${reference.digest.preview}` };
  }
  if (Boolean(answer.errorType) !== Boolean(reference.errorAny)) {
    return { status: "problem", problem: `tsc's error-type flag is ${answer.errorType}; the reference says errorAny ${reference.errorAny}` };
  }
  return { status: "reference" };
}

export function stats(values) {
  const xs = values.filter((v) => typeof v === "number" && Number.isFinite(v)).sort((a, b) => a - b);
  if (!xs.length) return null;
  const median = xs.length % 2 ? xs[(xs.length - 1) / 2] : (xs[xs.length / 2 - 1] + xs[xs.length / 2]) / 2;
  return { n: xs.length, min: xs[0], median, max: xs.at(-1) };
}

/**
 * A tsc request's time: the server's own processing time, bounded above by
 * the client's round trip (the server's clock is coarse on Windows and can
 * report more than the whole round trip took).
 */
export const tscRequestMs = (request) =>
  finite(request?.serverMs) && finite(request?.roundTripMs) ? Math.min(request.serverMs, request.roundTripMs) : null;

/** Per-invocation timing and memory of a probe arm, in milliseconds and bytes. */
export function probeMetrics(inv) {
  const r = inv.probe;
  const p = r?.probes?.[0];
  if (!p) return null;
  const sum = (...xs) => (xs.every((x) => typeof x === "number") ? xs.reduce((a, b) => a + b, 0) : null);
  if (r.tool === "verter") {
    const ms = (us) => (typeof us === "number" ? us / 1000 : null);
    const setup = ms(r.phases?.setup);
    const init = ms(r.phases?.init);
    const cold = ms(p.cold?.micros);
    return {
      engineStartMs: ms(r.phases?.engineStart),
      setupMs: setup,
      initMs: init,
      coldMs: cold,
      firstTypeMs: sum(setup, init, cold),
      warmMs: stats((p.warm ?? []).map((w) => ms(w.micros)))?.median ?? null,
      observeMs: ms(p.observeMicros),
      teardownMs: ms(r.phases?.teardown),
      peakBytes: r.afterRequests?.peakBytes ?? null,
      retainedBytes: r.afterRequests?.currentBytes ?? null,
      observePeakBytes: r.afterObserve?.peakBytes ?? null,
      cpuMs: typeof r.afterRequests?.cpuMicros === "number" ? r.afterRequests.cpuMicros / 1000 : null,
      memoryMetric: r.afterRequests?.metric ?? null,
      allocations: p.coldAllocations ?? null,
      retention: r.retention ?? null,
    };
  }
  const setup = finite(r.phases?.setupMs) && finite(r.phases?.setupRoundTripMs) ? Math.min(r.phases.setupMs, r.phases.setupRoundTripMs) : null;
  const init = tscRequestMs(r.init);
  const cold = tscRequestMs(p.cold);
  const engineStart =
    finite(r.phases?.engineStartMs) && finite(r.phases?.engineStartRoundTripMs)
      ? Math.min(r.phases.engineStartMs, r.phases.engineStartRoundTripMs)
      : null;
  return {
    spawnMs: r.phases?.spawnMs ?? null,
    engineStartMs: engineStart,
    setupMs: setup,
    setupRoundTripMs: r.phases?.setupRoundTripMs ?? null,
    initMs: init,
    coldMs: cold,
    coldRoundTripMs: p.cold?.roundTripMs ?? null,
    firstTypeMs: sum(setup, init, cold),
    warmMs: stats((p.warm ?? []).map(tscRequestMs))?.median ?? null,
    warmRoundTripMs: stats((p.warm ?? []).map((w) => w.roundTripMs))?.median ?? null,
    observeMs: p.observeMs ?? null,
    teardownMs: r.phases?.teardownMs ?? null,
    peakBytes: r.serverAfterRequests?.peakBytes ?? null,
    retainedBytes: r.serverAfterRequests?.currentBytes ?? null,
    observePeakBytes: r.serverAfterObserve?.peakBytes ?? null,
    cpuMs: typeof r.serverAfterRequests?.cpuMicros === "number" ? r.serverAfterRequests.cpuMicros / 1000 : null,
    memoryMetric: r.serverAfterRequests?.metric ?? null,
  };
}

/** Parse a `tsc -p --extendedDiagnostics` stdout. */
export function parseCli(stdout) {
  const num = (label) => {
    const m = new RegExp(`^${label}:\\s+([\\d,.]+)(K|s)?\\s*$`, "m").exec(stdout);
    if (!m) return null;
    const value = Number(m[1].replace(/,/g, ""));
    return m[2] === "K" ? value * 1024 : m[2] === "s" ? value * 1000 : value;
  };
  return {
    codes: [...new Set([...stdout.matchAll(/error TS(\d+)/g)].map((m) => Number(m[1])))].sort((a, b) => a - b),
    memoryUsedBytes: num("Memory used"),
    checkMs: num("Check time"),
    totalMs: num("Total time"),
    parseMs: num("Parse time"),
    bindMs: num("Bind time"),
    complete: /^Total time:/m.test(stdout),
  };
}

/**
 * The verdict of one metric between the two headline arms — a descriptive
 * rule, not a statistical test: a winner only when every measured repetition
 * of one arm beats every repetition of the other by more than `resolution`
 * (the metric's timer resolution); otherwise "overlap".
 */
export function verdict(verterValues, tscValues, resolution = 0) {
  const v = stats(verterValues);
  const t = stats(tscValues);
  if (!v || !t) return { verdict: "n/a", ratio: null };
  const ratio = v.median > 0 ? t.median / v.median : null;
  // Below the timer's resolution a median is not a measurement to divide.
  const ratioMeaningful = Math.min(v.median, t.median) >= resolution;
  if (v.max + resolution < t.min) return { verdict: "verter", ratio, ratioMeaningful };
  if (t.max + resolution < v.min) return { verdict: "tsc", ratio, ratioMeaningful };
  return { verdict: "overlap", ratio, ratioMeaningful };
}

/** Printed answers longer than this are embedded in results.json as their digest. */
export const ELIDE_TEXT_ABOVE = 4096;

/**
 * The copy of a probe record embedded in results.json: identical except
 * that an answer text longer than ELIDE_TEXT_ABOVE is replaced by its
 * canonical digest and the raw text's sha256. The raw record stays on disk;
 * the validator recomputes this compaction from it and compares.
 */
export function compactProbeRecord(record) {
  if (!record || !Array.isArray(record.probes)) return record;
  return {
    ...record,
    probes: record.probes.map((probe) => {
      const obs = probe.observation;
      if (!obs || typeof obs.text !== "string" || obs.text.length <= ELIDE_TEXT_ABOVE) return probe;
      let canonical = null;
      let canonicalError = null;
      try {
        canonical = canonicalDigest(obs.text);
      } catch (err) {
        canonicalError = String(err.message ?? err);
      }
      return {
        ...probe,
        observation: {
          ...obs,
          text: null,
          textElided: true,
          textLength: obs.text.length,
          textSha256: createHash("sha256").update(obs.text).digest("hex"),
          canonical,
          canonicalError,
        },
      };
    }),
  };
}
