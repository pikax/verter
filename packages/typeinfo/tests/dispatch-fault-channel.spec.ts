/**
 * Dispatch-fault error channel — TS wrapper completion of F1.
 *
 * The native carrier surfaces a genuine dispatch fault
 * (BudgetExceeded / UnstableState / AliasCycle / UnsupportedIntrinsic
 * / Other) through the result DTO's `error` field. This is inert
 * until the TS wrapper consumes it: `decodeResolveResult` must
 * surface the fault on the public API instead of silently projecting
 * it to a `{ type: undefined }` miss.
 *
 * REGRESSION — discriminating: the throw assertion FAILS against a
 * decoder that ignores the `error` field (it would return
 * `{ type: undefined }` and never throw). The miss assertion proves a
 * well-formed empty result is NOT mistaken for a fault.
 */

import { describe, expect, it } from "vitest";

import { decodeResolveResult, TypeResolutionFaultError } from "../src/decode.js";

describe("typeinfo dispatch-fault channel", () => {
  it("throws TypeResolutionFaultError when the native error channel is populated", () => {
    expect(() =>
      decodeResolveResult({
        typeExpr: null,
        auditRecord: null,
        error: 'AliasCycle { name: "Cyclic" }',
      }),
    ).toThrow(TypeResolutionFaultError);
  });

  it("carries the native fault description through the thrown error message", () => {
    let caught: unknown;
    try {
      decodeResolveResult({
        typeExpr: null,
        auditRecord: null,
        error: "BudgetExceeded { domain: ProjectionOperation }",
      });
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(TypeResolutionFaultError);
    expect((caught as Error).message).toContain("BudgetExceeded");
  });

  it("preserves the audit envelope on the thrown fault (audited fault stays audited)", () => {
    // The native carrier keeps the per-request audit record on the
    // `Err` arm. A fault with a NON-EMPTY audit buffer must surface
    // that record on the thrown error — dropping it would lose
    // observability for the exact requests operators most want to
    // inspect. Discriminating: FAILS against a decoder that throws
    // before decoding/attaching the audit record.
    const auditJson = JSON.stringify({
      request_id: "42",
      canonical_id: "/scope.ts",
      kind: "TypeResolution",
      from_cache: false,
    });
    let caught: unknown;
    try {
      decodeResolveResult({
        typeExpr: null,
        auditRecord: Buffer.from(auditJson, "utf-8"),
        error: 'AliasCycle { name: "Cyclic" }',
      });
    } catch (e) {
      caught = e;
    }
    expect(caught).toBeInstanceOf(TypeResolutionFaultError);
    const fault = caught as TypeResolutionFaultError;
    expect(fault.auditRecord).toBeDefined();
    expect(fault.auditRecord?.request_id).toBe("42");
    expect(fault.auditRecord?.kind).toBe("TypeResolution");
    expect(fault.auditRecord?.from_cache).toBe(false);
  });

  it("does NOT throw for a non-fault miss (error=null, typeExpr=null)", () => {
    // A well-formed request that resolved nothing is `type: undefined`,
    // never a thrown fault.
    const result = decodeResolveResult({
      typeExpr: null,
      auditRecord: null,
      error: null,
    });
    expect(result.type).toBeUndefined();
    expect(result.auditRecord).toBeUndefined();
  });
});
