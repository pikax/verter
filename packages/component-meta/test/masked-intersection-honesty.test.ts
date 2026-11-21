/**
 * Masked same-name intersection honesty at the NATIVE binding level.
 *
 * A `defineProps<{ x: string } & Bad>()` whose imported same-name
 * contributor `Bad.x` references a non-existent type must preserve that
 * stable unresolved reference as a completed symbolic carrier. The resolvable
 * local `x: string` arm must never mask it into a primitive-only success, and
 * neither intersection order may misclassify it as an operational budget
 * failure.
 */

import { describe, test, expect, afterAll } from "vitest";
import { join } from "path";
import { createCheckerByJson } from "../src/compat/checker.js";
import { shutdownMetaRuntime } from "../src/runtime/index.js";

const fixtureDir = join(__dirname, "fixtures");

afterAll(() => {
  shutdownMetaRuntime();
});

async function getChecker() {
  return createCheckerByJson(fixtureDir, {
    compilerOptions: { strict: true },
    include: ["**/*.vue", "**/*.ts"],
  });
}

describe("masked same-name intersection honesty (native)", () => {
  function expectStableSymbolicCarrier(meta: {
    _verter: {
      props: Array<{
        name: string;
        type?: unknown;
        typeExpansion?: unknown;
      }>;
    };
  }): void {
    const prop = meta._verter.props.find((candidate) => candidate.name === "x");
    expect(prop).toBeDefined();
    expect(prop?.typeExpansion).toEqual({
      executionStatus: "completed",
      exactness: "exactSymbolic",
      diagnostics: [],
    });
    expect(prop?.type).toEqual({
      kind: "intersection",
      types: expect.arrayContaining([
        { kind: "primitive", name: "string" },
        { kind: "ref", name: "MissingType" },
      ]),
    });
  }

  test("a stable unresolved imported contributor remains symbolic (local arm first)", async () => {
    const checker = await getChecker();
    const meta = await checker.getComponentMeta(join(fixtureDir, "MaskedIntersectionProps.vue"));
    expectStableSymbolicCarrier(meta);
  });

  test("a stable unresolved imported contributor remains symbolic (imported arm first)", async () => {
    const checker = await getChecker();
    const meta = await checker.getComponentMeta(
      join(fixtureDir, "MaskedIntersectionPropsReversed.vue"),
    );
    expectStableSymbolicCarrier(meta);
  });
});
