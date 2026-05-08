/**
 * Phase 4 Test #5 — Scratch-cache LRU eviction.
 *
 * Plan §6.4 row 5: configure `evaluateTypeExpressionCacheSize: 3`,
 * run 4 distinct expressions, verify the 1st is no longer cached
 * (re-issuing it produces a cold compute). Discriminating against
 * a no-op LRU stub.
 *
 * The signal is the audit record's `fromCache` flag — `true` on a
 * warm-cache hit, `false` on a cold compute. The plan §5.3 contract
 * guarantees the flag flips correctly.
 *
 * REGRESSION classification.
 */

import { describe, expect, it } from "vitest";

import { TypeInfoSession } from "../src/index.js";

describe("typeinfo scratch-cache LRU eviction", () => {
  it("evicts the oldest entry when cache is full and accepts new entries", () => {
    const session = new TypeInfoSession({
      root: "/fixtures",
      evaluateTypeExpressionCacheSize: 3,
    });
    session.host.upsert({
      inputId: "/fixtures/types.ts",
      source: `export type A = string;\nexport type B = number;\n`,
    });

    const exprs = ["Pick<A, 'length'>", "Pick<A, 'charAt'>", "Record<'k', A>", "Partial<B>"];

    // Cold compute on each — each `cacheable: true` request lands in
    // the scratch cache.
    const firstResults = exprs.map((expr) =>
      session.evaluateTypeExpression({
        scope: "/fixtures/types.ts",
        expression: expr,
        cacheable: true,
        mode: "expanded",
      }),
    );

    // Each call MUST emit an audit record (audit on by default).
    for (const r of firstResults) {
      expect(r.auditRecord).toBeDefined();
    }

    // The first three calls all wrote to the cache; the fourth call
    // (cap=3) evicted the oldest, which is index 0.
    // Re-evaluating expression[0] is therefore a COLD compute.
    const reFirst = session.evaluateTypeExpression({
      scope: "/fixtures/types.ts",
      expression: exprs[0],
      cacheable: true,
      mode: "expanded",
    });
    expect(reFirst.auditRecord).toBeDefined();
    // serde skips `from_cache: false` (skip_serializing_if), so the
    // absence of the field signals a cold compute.
    expect(reFirst.auditRecord?.from_cache ?? false, "evicted entry must miss the cache").toBe(
      false,
    );

    // Sanity: re-evaluating the most recently inserted (last) should
    // still hit the cache.
    const reLast = session.evaluateTypeExpression({
      scope: "/fixtures/types.ts",
      expression: exprs[3],
      cacheable: true,
      mode: "expanded",
    });
    expect(reLast.auditRecord?.from_cache, "fresh entry must hit the cache").toBe(true);

    session.host.close();
  });
});
