import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import { KNOWN_PRODUCT_GAP_ROUTE_KEYS, knownProductGapsForRoute } from "./knownProductGapManifest";
import { buildParityTestInventory, PARITY_FIXTURES } from "./parityTestInventory";
import { TYPE_PROVIDER_ROUTES } from "./routeInventory";

const libRoot = dirname(fileURLToPath(import.meta.url));

describe("known product-gap manifest", () => {
  it("declares every parity provider route and only required test IDs", () => {
    const expectedRoutes = PARITY_FIXTURES.flatMap((fixture) =>
      TYPE_PROVIDER_ROUTES.map((provider) => `${fixture}@${provider}`),
    ).sort();
    expect(KNOWN_PRODUCT_GAP_ROUTE_KEYS).toEqual(expectedRoutes);

    const inventory = buildParityTestInventory({
      suiteRoot: resolve(libRoot, "../suite/parity"),
      matrixCasesFile: resolve(libRoot, "matrixCases.ts"),
    });
    for (const fixture of PARITY_FIXTURES) {
      const required = new Set(inventory.testIdsByFixture[fixture]);
      for (const provider of TYPE_PROVIDER_ROUTES) {
        for (const [id, issue] of Object.entries(knownProductGapsForRoute(fixture, provider))) {
          expect(required.has(id), `${fixture}@${provider}: ${id}`).toBe(true);
          expect(issue).toMatch(/^ISSUE-[A-Za-z0-9_-]+$/);
        }
      }
    }
  });

  it("keeps the confidence invalidation regression sentinel fatal", () => {
    for (const provider of TYPE_PROVIDER_ROUTES) {
      expect(
        knownProductGapsForRoute("vue-parity", provider)[
          "confidence.invalidation.edit-introduces-unknown-prop"
        ],
      ).toBeUndefined();
    }
  });

  it("narrows allowed gaps with an explicit parity suite selector", () => {
    expect(
      knownProductGapsForRoute("vue-parity", "tsgo", [
        "vue.matrix.style-bind.accent.def",
        "vue.matrix.no-virtual.component-tag",
      ]),
    ).toEqual({
      "vue.matrix.style-bind.accent.def": "ISSUE-vue-matrix-style-bind-def",
    });
  });

  it("keeps the mixed cross-framework typing gap exact on every affected route", () => {
    for (const provider of TYPE_PROVIDER_ROUTES) {
      expect(
        knownProductGapsForRoute("mixed-parity", provider)["mixed.cross-import.vue-imports-svelte"],
      ).toBe("ISSUE-mixed-cross-import");
    }
  });

  it("accepts confirmed feature debt without absorbing readiness failures", () => {
    const svelteTsserver = knownProductGapsForRoute("svelte-parity", "tsserver");
    expect(svelteTsserver["strict.svelte.rest-props-opt-in"]).toBe(
      "ISSUE-svelte-strict-rest-props",
    );
    expect(svelteTsserver["svelte.matrix.strict-rest.clean"]).toBe(
      "ISSUE-svelte-matrix-strict-rest-clean",
    );
    expect(svelteTsserver["svelte.references.script-and-markup"]).toBeUndefined();
    expect(svelteTsserver["svelte.matrix.directives.if.hover"]).toBeUndefined();

    const vueTsgo = knownProductGapsForRoute("vue-parity", "tsgo");
    expect(vueTsgo["depth.rename.script-and-markup.min-two-edits"]).toBe(
      "ISSUE-depth-rename-apply",
    );
    expect(vueTsgo["generic.infer.good-clean-no-type-args"]).toBe("ISSUE-vue-generic-infer-good");
    expect(vueTsgo["generic.defaulted-t-string.no-annotation"]).toBe("ISSUE-vue-generic-default");
    expect(vueTsgo["vue.matrix.generic-infer.clean"]).toBe("ISSUE-vue-matrix-generic-infer-clean");
    expect(vueTsgo["vue.matrix.directives.v-if.hover"]).toBeUndefined();
    expect(vueTsgo["vue.matrix.slots.header-local.hover"]).toBeUndefined();
    expect(vueTsgo["vue.matrix.no-virtual.component-tag"]).toBeUndefined();
  });
});
