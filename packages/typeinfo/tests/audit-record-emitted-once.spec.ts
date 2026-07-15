/**
 * Audit record emitted exactly once per call.
 *
 * Every public host method emits exactly ONE `RequestAuditRecord`
 * per audited call. Calling `resolveSymbol` once produces one
 * record; calling it again increments the audit-store record count
 * by one (not zero, not two).
 *
 * REGRESSION — discriminates against a substrate that fails to
 * finalise the registration / publish to the records store exactly
 * once.
 */

import { describe, expect, it } from "vitest";

import { TypeInfoSession } from "../src/index.js";

interface AuditRecordsResponse {
  length: number;
}

function countAuditRecords(session: TypeInfoSession): number {
  // `getAuditRecords()` returns a JSON Buffer carrying an array of
  // records. Decoding the array length gives the audit-store size.
  const buf = session.host.getAuditRecords();
  if (!buf || buf.length === 0) {
    return 0;
  }
  const arr = JSON.parse(buf.toString("utf-8")) as unknown[];
  return Array.isArray(arr) ? arr.length : 0;
}

describe("typeinfo audit record emission", () => {
  it("emits exactly one RequestAuditRecord per resolveSymbol call", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: `export type Foo = { a: number };\n`,
    });

    const before = countAuditRecords(session);

    // Call #1
    const r1 = session.resolveSymbol("/fixtures/types.ts", "Foo", {
      mode: "expanded",
    });
    expect(r1.auditRecord).toBeDefined();
    expect(r1.auditRecord?.kind).toBe("TypeResolution");

    const afterOne = countAuditRecords(session);
    expect(afterOne, "first call must increment audit count by exactly 1").toBe(before + 1);

    // Call #2
    const r2 = session.resolveSymbol("/fixtures/types.ts", "Foo", {
      mode: "expanded",
    });
    expect(r2.auditRecord).toBeDefined();

    const afterTwo = countAuditRecords(session);
    expect(afterTwo, "second call must increment audit count by exactly 1 (not 0, not 2)").toBe(
      before + 2,
    );

    // Negative — distinct request_ids across the two records.
    expect(r1.auditRecord?.request_id).not.toBe(r2.auditRecord?.request_id);
    expect(r1.auditRecord?.request_id).toBeDefined();
    expect(r2.auditRecord?.request_id).toBeDefined();

    session.host.close();
  });

  it("emits exactly one RequestAuditRecord per evaluateTypeExpression call", () => {
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: `export type Foo = number;\n`,
    });

    const before = countAuditRecords(session);

    const r = session.evaluateTypeExpression({
      scope: "/fixtures/types.ts",
      expression: "Foo",
      mode: "expanded",
      cacheable: false,
    });
    expect(r.auditRecord).toBeDefined();

    const after = countAuditRecords(session);
    expect(after).toBe(before + 1);

    session.host.close();
  });

  it("each call carries a distinct request_id even on identical inputs", () => {
    // Sanity guard against a memoised audit record bypass: even when
    // the resolver returns a cached node, the per-request audit must
    // be a fresh record with a new monotonic id.
    const session = new TypeInfoSession({ root: "/fixtures" });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: `export type Foo = boolean;\n`,
    });

    const ids = new Set<string>();
    for (let i = 0; i < 3; i++) {
      const r = session.resolveSymbol("/fixtures/types.ts", "Foo", {
        mode: "expanded",
      });
      expect(r.auditRecord?.request_id).toBeDefined();
      ids.add(r.auditRecord!.request_id);
    }
    expect(ids.size, "three calls must produce three distinct request_ids").toBe(3);

    session.host.close();
  });
});
