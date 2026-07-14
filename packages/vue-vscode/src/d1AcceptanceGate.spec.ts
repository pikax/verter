import { describe, expect, it } from "vitest";

import { D1_GATE_ENV, d1GateRequested, evaluateD1Gate } from "./d1AcceptanceGate";

describe("d1GateRequested", () => {
  it("is false when unset / empty / falsey", () => {
    expect(d1GateRequested({})).toBe(false);
    expect(d1GateRequested({ [D1_GATE_ENV]: "" })).toBe(false);
    expect(d1GateRequested({ [D1_GATE_ENV]: "0" })).toBe(false);
    expect(d1GateRequested({ [D1_GATE_ENV]: "false" })).toBe(false);
  });
  it("is true when set to a truthy value", () => {
    expect(d1GateRequested({ [D1_GATE_ENV]: "1" })).toBe(true);
    expect(d1GateRequested({ [D1_GATE_ENV]: "yes" })).toBe(true);
  });
});

describe("evaluateD1Gate — HONEST GATE (requested-but-missing ⇒ FAIL, never skip)", () => {
  it("SKIP when the gate is not requested (excluded from the default matrix)", () => {
    const d = evaluateD1Gate({ requested: false, tsgoResolvable: false, shimPresent: false });
    expect(d.action).toBe("skip");
  });

  it("RUN when requested and every prerequisite is present", () => {
    const d = evaluateD1Gate({ requested: true, tsgoResolvable: true, shimPresent: true });
    expect(d.action).toBe("run");
  });

  // The discriminating honest-gate cases: a REQUESTED gate with a missing prereq is a
  // hard FAIL — a skip-pass here would silently launder a missing prerequisite.
  it("FAIL (not skip) when requested but tsgo is not resolvable", () => {
    const d = evaluateD1Gate({ requested: true, tsgoResolvable: false, shimPresent: true });
    expect(d.action).toBe("fail");
    if (d.action !== "fail") throw new Error("unreachable");
    expect(d.reason).toMatch(/tsgo/i);
    expect(d.reason).toMatch(/FAILURE|not a skip/i);
  });

  it("FAIL (not skip) when requested but the relay shim is not built", () => {
    const d = evaluateD1Gate({ requested: true, tsgoResolvable: true, shimPresent: false });
    expect(d.action).toBe("fail");
    if (d.action !== "fail") throw new Error("unreachable");
    expect(d.reason).toMatch(/shim/i);
    expect(d.reason).toMatch(/FAILURE|not a skip/i);
  });

  // NEGATIVE: a requested-but-missing gate must NEVER resolve to skip.
  it("never resolves a REQUESTED gate to skip, whatever the missing prereq", () => {
    for (const [tsgoResolvable, shimPresent] of [
      [false, false],
      [false, true],
      [true, false],
    ] as const) {
      const d = evaluateD1Gate({ requested: true, tsgoResolvable, shimPresent });
      expect(d.action).not.toBe("skip");
    }
  });
});
