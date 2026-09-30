// Interpretation of tsc 7.0.2's measured reference (expected.json): from the
// raw print the measurement recorded to the answer, its canonical digest and
// whether it is tsc's error-any. Kept apart from the measurement so the
// interpretation can be re-derived, audited and improved without measuring
// again. The answer is rebuilt from the parsed tree (never rendered and
// parsed again).

import { digestNode, isKeyword, normalize, parseType, unionMemberNodes } from "./canonical.mjs";

/** tsc's resource/complexity diagnostics: the answer beside one is tsc's fallback, not the type. */
export const RESOURCE_CODES = [2589, 2590, 2799, 2859];

/**
 * The answer a measured cell records:
 *   { digest, errorAny, codes }                               tsc answered;
 *   { gap, killed | truncated | unmeasurable, codes }         it did not (why).
 * `killed` is tsc exhausting the cap or the deadline; `truncated` and
 * `unmeasurable` are failures of the measurement, not of tsc.
 */
// ---------------------------------------------------------------- termination evidence
//
// The one policy by which a supervisor's kill of a process that IS the
// engine (a Verter probe, a `tsc -p` run) is read as the engine exhausting
// a resource. Shared by the benchmark arms and the reference measurement.

/** Backends whose memory accounting is the process tree's own (not a Linux cgroup's). */
export const OWN_ACCOUNTING_BACKENDS = ["windows-job-object", "macos-phys-footprint"];

const num = (x) => typeof x === "number" && Number.isFinite(x);

/**
 * A memory kill is the engine's when the backend accounts the tree's own
 * memory and its ACTUAL kill threshold (`killTriggerBytes`, recorded; never
 * inferred from the configured cap) reached the engine budget.
 */
export function memoryKillEvidence(rec, budgetBytes) {
  return (
    rec?.killedBy === "memory" &&
    OWN_ACCOUNTING_BACKENDS.includes(rec.backend) &&
    ["hard", "sampled"].includes(rec.containment) &&
    num(rec.killTriggerBytes) &&
    num(budgetBytes) &&
    rec.killTriggerBytes >= budgetBytes
  );
}

/**
 * A deadline kill is the engine's when the process itself ran for the whole
 * deadline: its wall time up to the moment termination began (the wall time
 * minus the recorded termination latency) is at least the deadline. Only a
 * process that is the engine from its start qualifies (a whole-program
 * `tsc -p` run).
 */
export function deadlineKillEvidence(rec, timeoutMs) {
  return (
    rec?.killedBy === "timeout" &&
    num(rec.wallMs) &&
    num(rec.terminationLatencyMs) &&
    num(timeoutMs) &&
    rec.wallMs - rec.terminationLatencyMs >= timeoutMs
  );
}

/**
 * Whether a measuring run's kill is established as tsc exhausting the
 * resource, by the shared policy against the measuring budget and deadline
 * its method declares.
 */
export function measurementKillAttributed(result) {
  const t = result.receipt?.termination;
  let method = null;
  try {
    method = JSON.parse(result.receipt?.method ?? "null");
  } catch {
    method = null;
  }
  if (!t || !method || t.killedBy !== result.killed) return false;
  if (result.killed === "memory")
    return memoryKillEvidence(t, num(method.memMb) ? method.memMb * 1024 * 1024 : null);
  if (result.killed === "timeout") return deadlineKillEvidence(t, method.timeoutMs);
  return false;
}

export function interpretMeasurement(result) {
  const codes = result.codes ?? [];
  if (result.killed) {
    if (!measurementKillAttributed(result))
      return {
        gap: `tsc -p was killed (${result.killed}) without evidence that tsc exhausted it`,
        unmeasurable: "unattributed kill",
        codes,
      };
    return { gap: `tsc -p exhausts resources (${result.killed})`, killed: result.killed, codes };
  }
  if (result.unmeasurable)
    return {
      gap: `unmeasurable: ${result.unmeasurable}`,
      unmeasurable: result.unmeasurable,
      codes,
    };
  if (result.printedOversize)
    return { gap: "tsc's print is too large to record", truncated: true, codes };
  let answer;
  if (result.never) answer = { k: "kw", name: "never" };
  else {
    if (typeof result.printed !== "string")
      return { gap: "no print recorded", unmeasurable: "no print", codes };
    const whole = normalize(parseType(result.printed));
    if (isKeyword(whole, "any") || isKeyword(whole, "never")) answer = whole;
    else {
      // The print is a union of one-element tuples, one per member. tsc's
      // printer elides a very large type even under noErrorTruncation,
      // leaving bare members (`any`) among the tuples: not the answer.
      const elements = [];
      for (const member of unionMemberNodes(result.printed)) {
        const single =
          member.k === "tuple" &&
          member.elements.length === 1 &&
          !member.elements[0].rest &&
          !member.elements[0].optional;
        if (!single) return { gap: "tsc prints the answer elided", truncated: true, codes };
        elements.push(member.elements[0].type);
      }
      answer = elements.length === 1 ? elements[0] : { k: "union", members: elements };
    }
  }
  const normalized = normalize(answer);
  const errorAny = isKeyword(normalized, "any") && codes.some((c) => RESOURCE_CODES.includes(c));
  return { digest: digestNode(normalized), errorAny, codes };
}
