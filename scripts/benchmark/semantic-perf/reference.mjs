// Interpretation of tsc 7.0.2's measured reference (expected.json): from the
// raw print the measurement recorded to the answer, its canonical digest and
// whether it is tsc's error-any. Kept apart from the measurement so the
// interpretation can be re-derived, audited and improved without measuring
// again.

import { canonicalDigest, canonicalType, renderNode, unionMemberNodes } from "./canonical.mjs";

/** tsc's resource/complexity diagnostics: the answer beside one is tsc's fallback, not the type. */
export const RESOURCE_CODES = [2589, 2590, 2799, 2859];

/**
 * The answer a measured cell records:
 *   { digest, text, errorAny, codes }                        tsc answered;
 *   { gap, killed | truncated | unmeasurable, codes }        it did not (why).
 * `killed` is tsc exhausting the cap or the deadline; `truncated` and
 * `unmeasurable` are failures of the measurement, not of tsc.
 */
export function interpretMeasurement(result) {
  const codes = result.codes ?? [];
  if (result.killed) return { gap: `tsc -p exhausts resources (${result.killed})`, killed: result.killed, codes };
  if (result.unmeasurable) return { gap: `unmeasurable: ${result.unmeasurable}`, unmeasurable: result.unmeasurable, codes };
  if (result.printedOversize) return { gap: "tsc's print is too large to record", truncated: true, codes };
  let text;
  if (result.never) text = "never";
  else {
    if (typeof result.printed !== "string") return { gap: "no print recorded", unmeasurable: "no print", codes };
    const bare = canonicalType(result.printed);
    if (bare === "any" || bare === "never") text = bare;
    else {
      // The print is a union of one-element tuples, one per member. tsc's
      // printer elides a very large type even under noErrorTruncation,
      // leaving bare members (`any`) among the tuples: not the answer.
      const elements = [];
      for (const member of unionMemberNodes(result.printed)) {
        const single = member.k === "tuple" && member.elements.length === 1 && !member.elements[0].rest && !member.elements[0].optional;
        if (!single) return { gap: "tsc prints the answer elided", truncated: true, codes };
        elements.push(renderNode(member.elements[0].type));
      }
      text = elements.join(" | ");
    }
  }
  const digest = canonicalDigest(text);
  const errorAny = canonicalType(text) === "any" && codes.some((c) => RESOURCE_CODES.includes(c));
  return { digest, text: digest.length <= 4000 ? text : null, errorAny, codes };
}

/** Whether a reference entry reports tsc exhausting a resource on the demand's program. */
export function referenceHitResourceLimit(answer) {
  return Boolean(answer?.killed) || (answer?.codes ?? []).some((c) => RESOURCE_CODES.includes(c));
}
