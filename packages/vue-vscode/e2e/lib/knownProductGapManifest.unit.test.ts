import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  KNOWN_PRODUCT_GAP_CANARY_ROUTE_KEYS,
  KNOWN_PRODUCT_GAP_ROUTE_KEYS,
  knownProductGapCanariesForRoute,
  knownProductGapsForRoute,
} from "./knownProductGapManifest";
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
    expect(svelteTsserver["shared.find.definition-then-refs-consistency"]).toBe(
      "ISSUE-find-def-refs-consistency",
    );
    expect(svelteTsserver["shared.find.js.exact-min-set"]).toBe("ISSUE-find-js-exact");
    expect(svelteTsserver["shared.rename.js.function"]).toBe("ISSUE-rename-js-function");
    expect(svelteTsserver["shared.rename.ts.markup-origin"]).toBe("ISSUE-rename-ts-markup");
    expect(svelteTsserver["svelte.references.script-and-markup"]).toBeUndefined();
    expect(svelteTsserver["svelte.matrix.directives.if.hover"]).toBeUndefined();

    const svelteTsgo = knownProductGapsForRoute("svelte-parity", "tsgo");
    expect(svelteTsgo["lsp.type-definition.binding"]).toBe("ISSUE-lsp-type-definition");

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

    const vueTsserver = knownProductGapsForRoute("vue-parity", "tsserver");
    expect(vueTsserver["depth.rename.script-and-markup.min-two-edits"]).toBe(
      "ISSUE-depth-rename-apply",
    );
  });

  it("declares canaries only for required test IDs on declared routes", () => {
    const inventory = buildParityTestInventory({
      suiteRoot: resolve(libRoot, "../suite/parity"),
      matrixCasesFile: resolve(libRoot, "matrixCases.ts"),
    });
    expect(KNOWN_PRODUCT_GAP_CANARY_ROUTE_KEYS.length).toBeGreaterThan(0);
    for (const route of KNOWN_PRODUCT_GAP_CANARY_ROUTE_KEYS) {
      expect(KNOWN_PRODUCT_GAP_ROUTE_KEYS).toContain(route);
      const [fixture, provider] = route.split("@") as [(typeof PARITY_FIXTURES)[number], string];
      const required = new Set(inventory.testIdsByFixture[fixture]);
      for (const [id, canary] of Object.entries(
        knownProductGapCanariesForRoute(fixture, provider),
      )) {
        expect(required.has(id), `${route}: ${id}`).toBe(true);
        expect(canary.issue).toMatch(/^ISSUE-[A-Za-z0-9_-]+$/);
        // A failure pattern that matches anything would tolerate any regression.
        expect(canary.failure.test(""), `${route}: ${id}`).toBe(false);
        expect(canary.failure.test("Diagnostics did not complete within 12000ms")).toBe(false);
      }
    }
  });

  it("never declares one test as both a skipped gap and a canary on a route", () => {
    for (const fixture of PARITY_FIXTURES) {
      for (const provider of TYPE_PROVIDER_ROUTES) {
        const skipped = knownProductGapsForRoute(fixture, provider);
        const both = Object.keys(knownProductGapCanariesForRoute(fixture, provider)).filter(
          (id) => skipped[id] !== undefined,
        );
        expect(both, `${fixture}@${provider}`).toEqual([]);
      }
    }
  });

  it("runs the plain-TS consumer cases as canaries on the editor-shared tsgo route only", () => {
    const issue = "ISSUE-shared-tsgo-plain-ts-consumer";
    const canaries = knownProductGapCanariesForRoute("vue-parity", "shared-tsgo");
    expect(
      Object.fromEntries(Object.entries(canaries).map(([id, row]) => [id, row.issue])),
    ).toEqual({
      "ide.complete.import-path-carrier": issue,
      "testing-api.vue.public-importer-hides-setup-bindings": issue,
      "vue.public-surface.consumer-source-documents-negative": issue,
      "vue.public-surface.no-secret-internal-on-component-hover": issue,
    });
    expect(KNOWN_PRODUCT_GAP_CANARY_ROUTE_KEYS).toEqual(["vue-parity@shared-tsgo"]);
    for (const fixture of PARITY_FIXTURES) {
      for (const provider of TYPE_PROVIDER_ROUTES) {
        if (fixture === "vue-parity" && provider === "shared-tsgo") continue;
        expect(knownProductGapCanariesForRoute(fixture, provider)).toEqual({});
      }
    }
  });
});
