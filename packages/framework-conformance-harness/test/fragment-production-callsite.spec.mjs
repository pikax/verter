// Self-test: the compileVueFixture PRODUCTION CALLSITE of fragment
// validation is kill-sensitive.
//
// Vocabulary discipline (arbitration): "mechanism logic" (does
// validateVueFragments work when called directly) is covered by
// test/fragment-validation.spec.mjs; THIS suite covers the "production
// callsite" — is the validator actually invoked on the real
// compileVueFixture path, and is its verdict actually consumed into the
// production failure result. Every previously-existing malformed-fragment
// test called the validator (or the assembler) directly, so removing the
// production call to fragment validation left the whole suite green — a
// kill mutation both the arbitration pass and the architecture review
// independently proved. This suite exists to make exactly that mutation
// fail.
//
// Method: the real official compiler never emits a malformed fragment, so a
// malformed-fragment VERDICT is injected at the module seam — the
// `validateVueFragments` import compileVueFixture's pipeline consumes — via
// vitest module mocking. Nothing about compileVueFixture, assembleAndValidate,
// or the real validator's logic is altered; the test drives the genuine
// production entry point over a genuine fixture and asserts the
// production-level failure result. Kill sensitivity: delete (or bypass) the
// production call to fragment validation and the injected verdict is never
// consulted, no fragment-error diagnostic reaches the production result,
// and this suite fails.

import { describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

vi.mock("../src/fragments.mjs", async (importOriginal) => {
  const original = await importOriginal();
  return {
    ...original,
    validateVueFragments: () => ({
      ok: false,
      fragments: [
        {
          kind: "render",
          parseOk: false,
          shapeOk: false,
          error: "planted malformed fragment (production-callsite probe)",
        },
      ],
    }),
  };
});

// Imported AFTER the mock declaration (vitest hoists vi.mock), so the
// production pipeline links against the injected validator seam.
import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";

describe("compileVueFixture production callsite of fragment validation", () => {
  it("a malformed-fragment verdict surfaces as a production-level failure result", () => {
    const fixturePath = "fixtures/vue/basic-interpolation.vue";
    const source = readFileSync(path.join(HARNESS_ROOT, fixturePath), "utf8");
    const artifact = compileVueFixture(source, fixturePath, {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    // Production-level failure: no artifact code, and the fragment verdict
    // reached the production diagnostics — enriched with the production
    // callsite's own `source` attribution, which only the compileVueFixture
    // pipeline adds.
    expect(artifact.code).toBeNull();
    const fragmentErrors = artifact.diagnostics.filter((d) => d.kind === "fragment-error");
    expect(fragmentErrors.length).toBe(1);
    expect(fragmentErrors[0].code).toBe("fragment-parse");
    expect(fragmentErrors[0].message).toContain(
      "render fragment invalid: planted malformed fragment (production-callsite probe)",
    );
    expect(fragmentErrors[0].source).toBe(fixturePath);
  });

  it("every backend's production path consults the fragment validator (vdom, vapor, ssr)", () => {
    const fixturePath = "fixtures/vue/basic-interpolation.vue";
    const source = readFileSync(path.join(HARNESS_ROOT, fixturePath), "utf8");
    for (const backend of ["vdom", "vapor", "ssr"]) {
      const artifact = compileVueFixture(source, fixturePath, {
        backend,
        sourceMap: false,
        isProd: false,
      });
      expect(artifact.code).toBeNull();
      expect(artifact.diagnostics.some((d) => d.kind === "fragment-error")).toBe(true);
    }
  });
});
