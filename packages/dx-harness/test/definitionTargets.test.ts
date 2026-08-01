/**
 * @ai-generated - Definition-target selection, including the same-line/different-
 * document near-miss a line-only comparison cannot detect.
 */
import { describe, expect, it } from "vitest";

import { definitionTargets, partitionDefinitionTargets } from "../src/definitionTargets.js";

const ENTRY = "file:///ws/App.vue";
const OTHER = "file:///ws/Other.vue";

describe("definitionTargets", () => {
  it("reads both Location and LocationLink shapes", () => {
    expect(definitionTargets({ uri: ENTRY, range: { start: { line: 11, character: 6 } } })).toEqual(
      [{ uri: ENTRY, line: 11, character: 6 }],
    );
    expect(
      definitionTargets([
        { targetUri: ENTRY, targetSelectionRange: { start: { line: 3, character: 1 } } },
      ]),
    ).toEqual([{ uri: ENTRY, line: 3, character: 1 }]);
  });

  it("drops entries with no usable uri or range instead of guessing", () => {
    expect(
      definitionTargets([null, {}, { uri: ENTRY }, { range: { start: { line: 1 } } }]),
    ).toEqual([]);
    expect(definitionTargets(null)).toEqual([]);
  });
});

describe("partitionDefinitionTargets", () => {
  it("separates a same-line target in a DIFFERENT document from an in-document one", () => {
    // The near-miss a line-only assertion accepts: the expected line number is
    // present, but in the wrong file. Comparing `targets.map(t => t.line)` against
    // the expected line reports success here; partitioning by document reports the
    // truth — nothing resolved in the driven entry.
    const targets = definitionTargets([
      { uri: OTHER, range: { start: { line: 11, character: 6 } } },
    ]);

    const { inDocument, elsewhere } = partitionDefinitionTargets(targets, ENTRY);

    // The line-only form would have been satisfied…
    expect(targets.map((target) => target.line)).toContain(11);
    // …while the document-aware form is not, and names the stray target.
    expect(inDocument).toEqual([]);
    expect(elsewhere).toEqual([{ uri: OTHER, line: 11, character: 6 }]);
  });

  it("keeps an in-document target and reports a stray one alongside it", () => {
    const targets = definitionTargets([
      { uri: ENTRY, range: { start: { line: 11, character: 6 } } },
      { uri: OTHER, range: { start: { line: 11, character: 0 } } },
    ]);

    const { inDocument, elsewhere } = partitionDefinitionTargets(targets, ENTRY);

    expect(inDocument).toEqual([{ uri: ENTRY, line: 11, character: 6 }]);
    expect(elsewhere).toEqual([{ uri: OTHER, line: 11, character: 0 }]);
  });
});
