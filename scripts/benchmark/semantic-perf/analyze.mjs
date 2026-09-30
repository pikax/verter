// Answer extraction, classification and statistics for the semantic
// benchmark. Pure functions over raw invocation records: the harness uses
// them to write its summary and the validator re-derives every summary field
// from the raw records with them, so a summary that disagrees with its own
// raw records fails validation.

import { createHash } from "node:crypto";

import { canonicalDigest } from "./canonical.mjs";

/** tsc's resource/complexity diagnostics: the answer beside one is tsc's fallback, not the type. */
export const RESOURCE_CODES = [2589, 2590, 2799, 2859];

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

/** Verter classes, strongest failure first. */
export const CLASSES = ["killed", "error", "refusal", "partial", "mismatch", "no-reference", "beyond-tsc", "matched"];

/** How an invocation ended, from the supervisor's point of view. */
export function invocationEnd(inv) {
  if (inv.skipped) return { kind: "killed", detail: "memory (skipped after a killed warmup)" };
  const rec = inv.supervisor;
  if (!rec) return { kind: "harness-failure", detail: inv.supervisorReadError ?? "no supervisor record" };
  if (!rec.launched) return { kind: "harness-failure", detail: `not launched: ${(rec.errors ?? []).join("; ")}` };
  if (rec.killedBy === "memory" || rec.killedBy === "timeout") return { kind: "killed", detail: rec.killedBy };
  if (rec.killedBy) return { kind: "harness-failure", detail: `killed by ${rec.killedBy}` };
  if ((rec.errors ?? []).length) return { kind: "harness-failure", detail: rec.errors.join("; ") };
  return { kind: "exited", exitCode: rec.exitCode };
}

/** The observed answer of one probe invocation (arm kind "probe"). */
export function probeAnswer(inv) {
  const end = invocationEnd(inv);
  if (end.kind !== "exited") return { end };
  if (end.exitCode !== 0) return { end: { kind: "child-failure", detail: `exit ${end.exitCode}` } };
  const result = inv.probe;
  if (!result) return { end: { kind: "child-failure", detail: inv.probeReadError ?? "no probe record" } };
  const probe = result.probes?.[0];
  if (!probe || result.probes.length !== 1) {
    return { end: { kind: "child-failure", detail: `expected exactly one probe record, got ${result.probes?.length ?? 0}` } };
  }
  const outcome = probe.cold?.outcome ?? { kind: "missing" };
  const warmKinds = (probe.warm ?? []).map((w) => w.outcome?.kind);
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
  return {
    end,
    alias: probe.alias,
    outcome,
    warmKinds,
    warmCount: warmKinds.length,
    digest,
    canonicalError,
    observeError: obs.error ?? null,
    errorType: obs.errorType ?? null,
    unionMembers: obs.unionMembers ?? null,
    unknownLeaves: obs.unknownLeaves ?? 0,
    unknownSamples: obs.unknownSamples ?? [],
    shape: obs.shape ?? null,
  };
}

function isBudgetFault(detail) {
  return /budget|limit|exhaust|depth/i.test(detail ?? "");
}

/**
 * Classify one Verter answer against tsc's measured reference.
 * `reference` is the catalog entry for the (scenario, setting).
 */
export function classifyVerterAnswer(answer, reference, beyondDigest, probeDigest = null) {
  const end = answer.end;
  if (end.kind === "killed") return { class: "killed", detail: end.detail };
  if (end.kind !== "exited") return { class: "error", detail: end.detail };
  if (answer.outcome.kind === "fault") {
    return isBudgetFault(answer.outcome.detail)
      ? { class: "refusal", detail: answer.outcome.detail }
      : { class: "error", detail: answer.outcome.detail };
  }
  if (answer.outcome.kind !== "value") return { class: "error", detail: `request outcome ${answer.outcome.kind}` };
  if (answer.warmKinds.some((k) => k !== "value")) return { class: "error", detail: "a warm repeat did not answer" };
  if (answer.observeError || !answer.digest) {
    return { class: "partial", detail: answer.observeError ?? answer.canonicalError ?? "no printable answer" };
  }
  if (answer.unknownLeaves > 0) {
    return { class: "partial", detail: `the answer holds ${answer.unknownLeaves} unmaterialised leaf/leaves (${answer.unknownSamples.join(", ")})` };
  }
  if (answer.shape === "conditional") return { class: "partial", detail: "the answer is an unevaluated conditional" };
  if (!referenceHasAnswer(reference)) {
    if (beyondDigest && answer.digest.sha256 === beyondDigest.sha256) return { class: "beyond-tsc", detail: "tsc exhausted its resources; Verter's answer is the constructed one" };
    return { class: "no-reference", detail: "tsc gives no answer to compare with" };
  }
  const tscFellBack = reference.codes.some((c) => RESOURCE_CODES.includes(c));
  if (!reference.errorAny && answer.digest.sha256 === reference.digest.sha256) {
    return { class: "matched", detail: tscFellBack ? "equal to tsc's answer at its resource limit" : "" };
  }
  if (tscFellBack && beyondDigest && answer.digest.sha256 === beyondDigest.sha256) {
    return { class: "beyond-tsc", detail: `tsc stops with TS${reference.codes.filter((c) => RESOURCE_CODES.includes(c)).join("/TS")}` };
  }
  if (probeDigest && answer.digest.sha256 === probeDigest.sha256) {
    return { class: "partial", detail: "the answer is the probe expression, unevaluated" };
  }
  return { class: "mismatch", detail: `answered ${answer.digest.preview}, tsc ${reference.errorAny ? "error-any" : reference.digest.preview}` };
}

/**
 * Whether a tsc API answer reproduces the measured reference. A mismatch
 * means the catalog or the tsc arm is wrong: the run is invalid.
 */
export function tscAnswerProblem(answer, reference) {
  const end = answer.end;
  if (end.kind === "killed") return null;
  if (end.kind !== "exited") return `tsc invocation failed: ${end.detail}`;
  if (answer.outcome.kind !== "value") return `tsc request outcome ${answer.outcome.kind} ${answer.outcome.detail ?? ""}`;
  if (!reference) return "no measured reference for this scenario and setting";
  // A reference whose measurement tsc could not finish is judged by
  // tscAnswerStatus instead: the API's demand can succeed where the CLI's
  // whole-program check exhausts the cap.
  if (!referenceHasAnswer(reference)) return null;
  if (!answer.digest) return `tsc printed no answer: ${answer.observeError ?? answer.canonicalError}`;
  if (answer.digest.sha256 !== reference.digest.sha256) {
    return `tsc answered ${answer.digest.preview}; the measured reference is ${reference.digest.preview}`;
  }
  if (Boolean(answer.errorType) !== Boolean(reference.errorAny)) {
    return `tsc's error-type flag is ${answer.errorType}; the reference says errorAny ${reference.errorAny}`;
  }
  return null;
}

/**
 * The tsc arm's standing for one invocation:
 *   "killed"        the supervisor killed it (a valid observation);
 *   "reference"     it reproduces the measured reference;
 *   "by-construction"  the CLI measurement exhausted resources, but the API
 *                   answered, and its answer is the scenario's constructed one;
 *   "unverified"    the CLI measurement exhausted resources and the API's
 *                   answer cannot be checked (no constructed answer, or a
 *                   different one): never compared;
 *   "problem"       it contradicts the reference or failed (fails validation).
 */
export function tscAnswerStatus(answer, reference, beyondDigest) {
  if (answer.end.kind === "killed") return { status: "killed" };
  const problem = tscAnswerProblem(answer, reference);
  if (problem) return { status: "problem", problem };
  if (reference && !referenceHasAnswer(reference)) {
    if (!answer.digest) return { status: "problem", problem: `tsc printed no answer: ${answer.observeError ?? answer.canonicalError}` };
    if (!answer.errorType && beyondDigest && answer.digest.sha256 === beyondDigest.sha256) return { status: "by-construction" };
    return { status: "unverified" };
  }
  return { status: "reference" };
}

export function stats(values) {
  const xs = values.filter((v) => typeof v === "number" && Number.isFinite(v)).sort((a, b) => a - b);
  if (!xs.length) return null;
  const median = xs.length % 2 ? xs[(xs.length - 1) / 2] : (xs[xs.length / 2 - 1] + xs[xs.length / 2]) / 2;
  return { n: xs.length, min: xs[0], median, max: xs.at(-1) };
}

/** Per-invocation timing and memory of a probe arm, in milliseconds and bytes. */
export function probeMetrics(inv) {
  const r = inv.probe;
  const p = r?.probes?.[0];
  if (!p) return null;
  if (r.tool === "verter") {
    const ms = (us) => (typeof us === "number" ? us / 1000 : null);
    const setup = ms(r.phases?.setup);
    const init = ms(r.phases?.init);
    const cold = ms(p.cold?.micros);
    return {
      setupMs: setup,
      initMs: init,
      coldMs: cold,
      firstAnswerMs: setup !== null && init !== null && cold !== null ? setup + init + cold : null,
      warmMs: stats((p.warm ?? []).map((w) => ms(w.micros)))?.median ?? null,
      observeMs: ms(p.observe_micros ?? p.observeMicros),
      teardownMs: ms(r.phases?.teardown),
      peakBytes: r.afterRequests?.peakBytes ?? null,
      retainedBytes: r.afterRequests?.currentBytes ?? null,
      peakResidentBytes: r.afterRequests?.peakResidentBytes ?? null,
      cpuMs: typeof r.afterRequests?.cpuMicros === "number" ? r.afterRequests.cpuMicros / 1000 : null,
      memoryMetric: r.afterRequests?.metric ?? null,
      allocations: p.coldAllocations ?? null,
      retention: r.retention ?? null,
    };
  }
  const setup = r.phases?.setupMs ?? null;
  const init = r.init?.serverMs ?? null;
  const cold = p.cold?.serverMs ?? null;
  return {
    spawnMs: r.phases?.spawnMs ?? null,
    setupMs: setup,
    initMs: init,
    coldMs: cold,
    coldRoundTripMs: p.cold?.roundTripMs ?? null,
    firstAnswerMs: setup !== null && init !== null && cold !== null ? setup + init + cold : null,
    warmMs: stats((p.warm ?? []).map((w) => w.serverMs))?.median ?? null,
    warmRoundTripMs: stats((p.warm ?? []).map((w) => w.roundTripMs))?.median ?? null,
    observeMs: p.observeMs ?? null,
    teardownMs: r.phases?.teardownMs ?? null,
    peakBytes: r.serverAfterRequests?.peakBytes ?? null,
    retainedBytes: r.serverAfterRequests?.currentBytes ?? null,
    peakResidentBytes: r.serverAfterRequests?.peakResidentBytes ?? null,
    cpuMs: typeof r.serverAfterRequests?.cpuMicros === "number" ? r.serverAfterRequests.cpuMicros / 1000 : null,
    memoryMetric: r.serverAfterRequests?.metric ?? null,
    diagnosticCodes: (r.diagnostics ?? []).map((d) => d.code),
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
 * The verdict of one metric between the two headline arms: a winner only
 * when every measured repetition of one arm beats every repetition of the
 * other (the ranges do not overlap) and the gap exceeds the timer's
 * resolution; otherwise "overlap".
 */
export function verdict(verterValues, tscValues, resolution = 0) {
  const v = stats(verterValues);
  const t = stats(tscValues);
  if (!v || !t) return { verdict: "n/a", ratio: null };
  const ratio = v.median > 0 ? t.median / v.median : null;
  if (v.max + resolution < t.min) return { verdict: "verter", ratio };
  if (t.max + resolution < v.min) return { verdict: "tsc", ratio };
  return { verdict: "overlap", ratio };
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

/** Whether the measured reference holds tsc's answer (not killed, truncated or unmeasurable). */
export function referenceHasAnswer(reference) {
  return Boolean(reference && !reference.killed && !reference.truncated && !reference.unmeasurable && reference.digest);
}

/** Why a measured reference holds no answer, or null. */
export function referenceGap(reference) {
  if (!reference) return "no measurement";
  if (reference.killed) return `tsc -p exhausts resources (${reference.killed})`;
  if (reference.truncated) return "tsc prints the answer elided";
  if (reference.unmeasurable) return `unmeasurable: ${reference.unmeasurable}`;
  return null;
}
