// Answers of the open-source type-checker arms (`--oss`): reading a
// whole-program run's output as an answer to the demand, and classifying it
// against tsc 7.0.2's measured reference.
//
// Every OSS arm answers the demand the way tsc's reference was measured (see
// measure-expected.mjs): the tool checks the scenario plus MEASURING_SUFFIX,
// and the head line of the diagnostic on the measuring assignment prints the
// probe's type as a union of one-element tuples. So the tools, and the
// `tsc-measure` reference arms beside them, do the SAME work on the SAME
// bytes, and the reading and interpretation are the reference's own code
// (parseMeasurement, interpretMeasurement), never a per-tool guess. A tool
// whose output cannot be read that way has no comparable answer: its row is
// `unreadable`, never a match.

import { parseMeasurement } from "../measure-expected.mjs";
import { interpretMeasurement, RESOURCE_CODES } from "../reference.mjs";

/** The classes of an OSS arm's answer, worst first. */
export const OSS_CLASSES = [
  "unavailable",
  "killed",
  "error",
  "unverified",
  "unreadable",
  "mismatch",
  "no-reference",
  "beyond-tsc",
  "matched",
];

/**
 * Read one whole-program output as an answer. `format` is the tool's
 * diagnostic format; only "tsc" (the `file(line,col): error TSnnnn: message`
 * lines tsc prints when its output is not a terminal) is readable. Returns
 * `{ answer }` (the interpreted measurement: `digest`, `errorAny`, `codes`,
 * or a `gap`) or `{ unreadable }` (why the output is not an answer).
 */
export function readOssAnswer(format, stdout, source) {
  if (format !== "tsc")
    return { unreadable: `the tool prints diagnostics in its own format (${format}), not tsc's` };
  let parsed;
  try {
    parsed = parseMeasurement(stdout, source);
  } catch (err) {
    return { unreadable: String(err.message ?? err).slice(0, 300) };
  }
  let answer;
  try {
    answer = interpretMeasurement({
      never: parsed.never,
      codes: parsed.codes,
      printed: parsed.printed ?? undefined,
    });
  } catch (err) {
    return {
      unreadable: `the printed answer is not type syntax: ${String(err.message ?? err).slice(0, 200)}`,
    };
  }
  if (answer.gap)
    return {
      unreadable: answer.truncated ? "the printed answer is elided" : answer.gap,
      codes: parsed.codes,
    };
  return { answer };
}

const sameCodes = (a, b) =>
  JSON.stringify([...new Set(a)].sort((x, y) => x - y)) ===
  JSON.stringify([...new Set(b)].sort((x, y) => x - y));

/**
 * Classify one OSS invocation's answer. `end` is how the invocation ended
 * (analyze.mjs invocationEnd, with a non-zero exit outside tsc's 0/1/2 read as
 * a failure); `read` is readOssAnswer's result; `ctx.reference` the
 * interpreted reference; `ctx.beyond` the digest of the scenario's
 * constructed answer past tsc's limit.
 *
 * `matched` needs the canonical answer AND the set of other diagnostic codes
 * to equal the reference's: a whole-program run's answer is its type and its
 * diagnostics, and a tool that reports an error tsc does not (or misses one
 * tsc reports) did different work. Where tsc answers its error-any beside a
 * resource diagnostic, only the same error-any beside the same codes
 * matches: the tool stopped where tsc stops.
 */
export function classifyOssAnswer(end, read, ctx = {}) {
  const { reference = null, beyond = null } = ctx;
  if (end.kind === "killed") return { class: "killed", detail: end.detail };
  if (end.kind === "unattributed-kill") return { class: "unverified", detail: end.detail };
  if (end.kind !== "exited") return { class: "error", detail: end.detail ?? end.kind };
  if (![0, 1, 2].includes(end.exitCode)) return { class: "error", detail: `exit ${end.exitCode}` };
  if (read.unreadable) return { class: "unreadable", detail: read.unreadable };
  const answer = read.answer;
  if (!reference || reference.gap || !reference.digest) {
    if (reference?.killed && beyond && answer.digest.sha256 === beyond.sha256)
      return {
        class: "beyond-tsc",
        detail: "tsc exhausts the cap on this demand; the answer is the constructed one",
      };
    return { class: "no-reference", detail: reference?.gap ?? "no measured reference" };
  }
  const limitCodes = reference.codes.filter((c) => RESOURCE_CODES.includes(c));
  const sameAnswer =
    answer.errorAny === reference.errorAny && answer.digest.sha256 === reference.digest.sha256;
  if (sameAnswer && sameCodes(answer.codes, reference.codes))
    return {
      class: "matched",
      detail: reference.errorAny
        ? "reproduces tsc's error-any fallback at its resource limit"
        : limitCodes.length
          ? "equal to tsc's answer at its resource limit"
          : "",
    };
  if (limitCodes.length && beyond && answer.digest.sha256 === beyond.sha256)
    return { class: "beyond-tsc", detail: `tsc stops with TS${limitCodes.join("/TS")}` };
  const codes = (c) => (c.length ? c.map((x) => `TS${x}`).join(",") : "none");
  if (sameAnswer)
    return {
      class: "mismatch",
      detail: `same answer, other diagnostics: ${codes(answer.codes)}; tsc ${codes(reference.codes)}`,
    };
  return {
    class: "mismatch",
    detail: `answered ${answer.errorAny ? "error-any" : answer.digest.preview}${answer.codes.length ? ` + ${codes(answer.codes)}` : ""}, tsc ${reference.errorAny ? "error-any" : reference.digest.preview}${reference.codes.length ? ` + ${codes(reference.codes)}` : ""}`,
  };
}

/**
 * The reference arm's standing (`tsc-measure`, `tsc-measure-1`): it runs the
 * reference's own method, so it must reproduce the reference exactly, or the
 * run fails validation (the reference or the arm is wrong). Returns
 * `{ status: "reference" | "killed" | "unverified" | "no-reference" }` or
 * `{ status: "problem", problem }`.
 */
export function tscMeasureStatus(end, read, reference) {
  if (end.kind === "killed") return { status: "killed", detail: end.detail };
  if (end.kind === "unattributed-kill") return { status: "unverified", detail: end.detail };
  if (end.kind !== "exited")
    return { status: "problem", problem: `tsc failed: ${end.detail ?? end.kind}` };
  if (![0, 1, 2].includes(end.exitCode))
    return { status: "problem", problem: `tsc exited ${end.exitCode}` };
  if (!reference) return { status: "no-reference" };
  if (reference.gap) {
    // The reference holds no answer (tsc exhausted it, or its print was
    // elided): the arm must not claim one either.
    return read.unreadable || read.answer?.errorAny
      ? { status: "no-reference", detail: reference.gap }
      : {
          status: "unverified",
          detail: `the reference has no answer (${reference.gap}); this run printed one`,
        };
  }
  if (read.unreadable)
    return { status: "problem", problem: `tsc's output is unreadable: ${read.unreadable}` };
  const a = read.answer;
  if (a.digest.sha256 !== reference.digest.sha256 || a.errorAny !== reference.errorAny)
    return {
      status: "problem",
      problem: `tsc answered ${a.errorAny ? "error-any" : a.digest.preview}; the measured reference is ${reference.errorAny ? "error-any" : reference.digest.preview}`,
    };
  if (!sameCodes(a.codes, reference.codes))
    return {
      status: "problem",
      problem: `tsc reported TS${a.codes.join(",TS")}; the reference TS${reference.codes.join(",TS")}`,
    };
  return { status: "reference" };
}
