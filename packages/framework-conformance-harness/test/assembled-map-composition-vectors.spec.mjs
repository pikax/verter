// Reproduction of the complete layer-2 seed vector suite by the independent
// JavaScript reference.
//
// AMD-008 requires the reference to reproduce every vector EXACTLY. Revision 2
// of the vectors file completed every `input` to a real §3.3 `AssembleInput`
// DTO and every `expected` to the complete `composeAssembledVueMainModule`
// result shape (positive: `{outcome, code, map, mapAbsentMembers, segments}`;
// fail-closed: `{outcome, family, code, fragment}` or
// `{outcome: "MissingRequiredInputMap", fragment}`) — so every vector is now
// driven straight through the mandated entry point with no adaptation layer.
//
// ONE DOCUMENTED DEVIATION, V4. §9.1 of the frozen layer-1 spec: "V4's
// expected segment sequence is not what this specification produces" in
// revision 1 of the vectors file — revision 2 already corrects V4's `expected`
// to layer 1's full five-segment result, so V4 now needs no special handling
// here; it is asserted like every other vector. The historical three-segment
// reading is preserved only as a comment for context, not as a second
// expectation.

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

import { composeAssembledVueMainModule } from "../src/assembled-map-composition-reference.mjs";

const SUITE = JSON.parse(
  readFileSync(
    new URL("../vectors/assembled-map-composition.vectors.json", import.meta.url),
    "utf8",
  ),
);

const vector = (id) => {
  const found = [...SUITE.vectors, ...SUITE.failClosedVectors].find((entry) => entry.id === id);
  if (found === undefined) throw new Error(`vector ${id} is absent from the suite`);
  return found;
};

// Every id actually driven through `composeAssembledVueMainModule` by this
// file's tests. The trailing coverage suite asserts this set exactly matches
// the suite's own id inventory, so a vector added to the JSON without a test
// that drives it is a structural failure here — not a silent skip. (Relies on
// vitest's default in-file declaration-order execution; a filtered run — e.g.
// `it.only` — legitimately fails the coverage assertion.)
const EXERCISED = new Set();

/**
 * Drive a vector straight through the mandated entry point and compare
 * against its `expected`.
 *
 * A `"composed"` vector's `expected` carries one bookkeeping-only member,
 * `mapAbsentMembers` — the fixture's own list of which `MapArtifact` members
 * must NOT be keys of `map`, checked here directly against the real result
 * rather than trusted from the fixture — which is not part of the actual
 * result shape and is stripped before comparison. The real result also
 * carries `provenance` (which fragment produced each segment), which the
 * vectors format does not encode as part of `expected` — it is compared
 * explicitly by individual tests that care about it (V4), not generically
 * here, so it is excluded from the blanket equality check.
 */
function assertVector(id) {
  const spec = vector(id);
  const result = composeAssembledVueMainModule(spec.input);
  EXERCISED.add(id);
  if (spec.expected.outcome === "composed") {
    const { mapAbsentMembers, ...expected } = spec.expected;
    const { provenance, ...comparable } = result;
    expect(comparable).toEqual(expected);
    for (const member of mapAbsentMembers ?? []) {
      expect(result.map, `${id}: map must not carry '${member}'`).not.toHaveProperty(member);
    }
  } else {
    expect(result).toEqual(spec.expected);
  }
  return result;
}

const assertComposed = assertVector;
const assertFailClosed = assertVector;

describe("layer-2 seed vectors — positive (§9)", () => {
  it("V1: rename token geometry and a TERMINAL removal, which has no following-chunk token", () => {
    assertComposed("V1");
  });

  it("V2: a NON-TERMINAL removal emits a following-chunk token, SOURCELESS on a line with no applicable segment", () => {
    const result = assertComposed("V2");
    // The discriminating fact: the transition segment at (1,0) is sourceless
    // rather than inheriting an authored position from the previous line (§5.4).
    const transition = result.segments.find((s) => s.genLine === 1 && s.genCol === 0);
    expect(transition).toBeDefined();
    expect(transition.srcIdx).toBeNull();
  });

  it("V3: two segments sharing one generated coordinate keep their wire order", () => {
    const result = assertComposed("V3");
    // Order-significance: the two coincident segments are distinguished only
    // by which one is LAST in the sequence — a multiset or sorted comparison
    // would not catch a swap.
    const coincident = result.segments.filter((s) => s.genLine === 0 && s.genCol === 0);
    expect(coincident.length).toBeGreaterThanOrEqual(2);
  });

  it("V4: layer 1's full five-segment sequence, including both BR-3 boundary segments (§9.1)", () => {
    const result = assertComposed("V4");
    // Historical note: an earlier revision of this vector stated only the
    // three chained (non-boundary) segments; layer 1 §9.1 requires the two
    // additional sourceless `AssemblyBoundary` segments this asserts.
    const boundaries = result.provenance.filter((p) => p === "AssemblyBoundary");
    expect(boundaries).toHaveLength(2);
    expect(result.segments).toHaveLength(5);
  });

  it("V5: columns are UTF-16 code units — not code points and not UTF-8 bytes", () => {
    const result = assertComposed("V5");
    // The three readings the vector separates: 11 (UTF-16), 10 (code points),
    // 13 (UTF-8 bytes). Only the first may appear.
    expect(result.segments.some((s) => s.genCol === 10)).toBe(false);
    expect(result.segments.some((s) => s.genCol === 13)).toBe(false);
  });

  it("V6: a CR retained before a LF occupies a real column; lines split on LF only", () => {
    assertComposed("V6");
  });

  it("V7: a sourceless segment is a BARRIER, not a transparent hole", () => {
    const result = assertComposed("V7");
    // An implementation that skipped past a sourceless segment to find the
    // nearest source-bearing one would fabricate provenance here.
    expect(result.segments.slice(1).every((s) => s.srcIdx === null)).toBe(true);
  });

  it("V8: a mid-line removal joins the following line onto the match's prefix", () => {
    assertComposed("V8");
  });

  it("V9: a source-bearing old-end transition, contrasting V2's sourceless case", () => {
    assertComposed("V9");
  });

  it("V10: two distinct segments strictly inside a rename range are both dropped", () => {
    assertComposed("V10");
  });

  it("V11: multiple same-line rename replacements accumulate their column shift", () => {
    assertComposed("V11");
  });

  it("V12: N-ary coincident segments at an overwrite's start column", () => {
    assertComposed("V12");
  });

  it("V13: BR-3 fires at the assembled-module level for a fragment ending in LF", () => {
    assertComposed("V13");
  });

  it("V14: BR-3 does NOT fire for the empty-present-fragment case (§6.4 case 4′)", () => {
    assertComposed("V14");
  });

  it("V15: sourceRoot, sourcesContent, and ignoreList composition", () => {
    assertComposed("V15");
  });

  it("V16: a non-zero fragment placement", () => {
    assertComposed("V16");
  });

  it("V17: sourceMapRequested false carries no map", () => {
    const result = assertComposed("V17");
    expect(result.map).toBeNull();
    expect(result.segments).toBeNull();
  });

  it("V18: zero contributing maps still yields a requested, empty artifact", () => {
    const result = assertComposed("V18");
    expect(result.map).not.toBeNull();
  });

  it("V19: the template fragment is never rewritten, even when its bytes literally contain both rewrite spellings", () => {
    assertComposed("V19");
  });

  it("V20: a table-less contributing map (empty sources/names, empty mappings) still composes", () => {
    assertComposed("V20");
  });

  it("V21: an ordinary non-empty, non-LF-terminated contributing fragment — BR-3 must not fire", () => {
    assertComposed("V21");
  });

  it("V22: sourceRoot absent and sourceRoot: null normalise to the same value and agree", () => {
    assertComposed("V22");
  });

  it("V23: the write-manifest's SSR context wrapper (W-17)", () => {
    assertComposed("V23");
  });
});

describe("layer-2 seed vectors — fail-closed (§9)", () => {
  const ids = SUITE.failClosedVectors.map((entry) => entry.id);

  it("the suite enumerates at least one vector per §4.4 family", () => {
    const families = new Set(
      SUITE.failClosedVectors
        .filter((entry) => entry.expected.outcome === "UncomposableInputMap")
        .map((entry) => entry.expected.family),
    );
    for (const family of ["U1", "U2", "U3", "U4", "U5", "U6", "U7", "U8"]) {
      expect(families, `no fail-closed vector covers family ${family}`).toContain(family);
    }
  });

  for (const id of ids) {
    it(`${id}: ${vector(id).intent}`, () => {
      assertFailClosed(id);
    });
  }
});

// Declared LAST so it runs after every vector test above (vitest executes a
// file's tests sequentially in declaration order). The expected inventory is
// the suite's OWN id list — no count is hardcoded — so this is the structural
// tripwire that turns "a vector exists in the JSON but nothing here drives it"
// into a failure with the missing ids named.
describe("suite coverage — every vector was actually exercised", () => {
  it("the exercised id set exactly matches the suite's own id inventory", () => {
    const suiteIds = [...SUITE.vectors, ...SUITE.failClosedVectors].map((entry) => entry.id);
    expect(new Set(suiteIds).size, "the suite declares a duplicate vector id").toBe(
      suiteIds.length,
    );

    const missing = suiteIds.filter((id) => !EXERCISED.has(id));
    const extra = [...EXERCISED].filter((id) => !suiteIds.includes(id));
    expect(
      missing,
      `suite vectors never driven through the entry point: ${missing.join(", ")}`,
    ).toEqual([]);
    expect(extra, `exercised ids absent from the suite: ${extra.join(", ")}`).toEqual([]);
    expect(EXERCISED.size).toBe(suiteIds.length);
  });
});
